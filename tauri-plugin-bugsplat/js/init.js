// Injected into every webview by tauri-plugin-bugsplat before page scripts run.
// Reports uncaught errors and unhandled promise rejections to BugSplat through Rust, using
// bugsplat-js's exception-chain format, and exposes window.BugSplat.post(error, attributes).
(function () {
  if (window.__BUGSPLAT__) return;
  var MAX_REPORTS = 20;
  var MAX_LINKED = 100;
  var sent = 0;

  function entry(value, id, parentId, source) {
    var isError = value instanceof Error;
    var obj = value !== null && typeof value === 'object';
    var properties = {};
    if (obj) {
      Object.keys(value).forEach(function (k) {
        if (['name', 'message', 'stack', 'cause', 'errors'].indexOf(k) !== -1) return;
        try { properties[k] = JSON.parse(JSON.stringify(value[k])); } catch (_) { properties[k] = String(value[k]); }
      });
    }
    return {
      id: id,
      parentId: parentId,
      source: source,
      type: isError ? (value.name || (value.constructor && value.constructor.name) || 'Error')
        : obj ? ((value.constructor && value.constructor.name) || 'Object') : typeof value,
      message: isError ? String(value.message) : obj && typeof value.message === 'string' ? value.message : String(value),
      stack: isError && value.stack ? String(value.stack) : undefined,
      properties: properties
    };
  }

  // Depth-first over Error.cause and AggregateError.errors, cause first (as bugsplat-js does).
  function chain(error) {
    var out = [entry(error, 0, null, null)];
    var seen = [error];
    function walk(value, parentId) {
      if (value === null || typeof value !== 'object') return;
      var links = [];
      if ('cause' in value && value.cause !== undefined) links.push(['cause', value.cause]);
      if (Array.isArray(value.errors)) value.errors.forEach(function (e, i) { links.push(['errors[' + i + ']', e]); });
      links.forEach(function (link) {
        if (out.length > MAX_LINKED || seen.indexOf(link[1]) !== -1) return;
        seen.push(link[1]);
        var id = out.length;
        out.push(entry(link[1], id, parentId, link[0]));
        walk(link[1], id);
      });
    }
    walk(error, 0);
    return out;
  }

  function post(error, attributes) {
    if (sent >= MAX_REPORTS) return Promise.resolve(null);
    sent++;
    var attrs = { url: String(location.href) };
    Object.keys(attributes || {}).forEach(function (k) { attrs[k] = String(attributes[k]); });
    return window.__TAURI_INTERNALS__
      .invoke('plugin:bugsplat|report_error', { report: { exceptions: chain(error), attributes: attrs } })
      .catch(function (e) { console.warn('BugSplat: report failed', e); return null; });
  }

  window.addEventListener('error', function (event) {
    var error = event.error !== undefined && event.error !== null ? event.error : new Error(event.message || 'Script error');
    post(error, { 'js.source': (event.filename || '') + ':' + event.lineno + ':' + event.colno });
  });
  window.addEventListener('unhandledrejection', function (event) {
    post(event.reason, { 'js.kind': 'unhandledrejection' });
  });

  window.__BUGSPLAT__ = { chain: chain };
  window.BugSplat = { post: post };
})();
