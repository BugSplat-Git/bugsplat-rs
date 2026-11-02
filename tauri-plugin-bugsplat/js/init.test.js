// node --test tauri-plugin-bugsplat/js/
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, 'init.js'), 'utf8');

function load() {
  const listeners = {};
  const calls = [];
  const window = {
    addEventListener: (type, fn) => { (listeners[type] ||= []).push(fn); },
    __TAURI_INTERNALS__: {
      invoke: (cmd, args) => { calls.push({ cmd, args }); return Promise.resolve({ crashId: 1 }); },
    },
  };
  const context = { window, location: { href: 'tauri://localhost/index.html' }, console, Error, TypeError, AggregateError, JSON, Object, Array, String, Promise };
  vm.createContext(context);
  vm.runInContext(source, context);
  const fire = (type, event) => listeners[type].forEach(fn => fn(event));
  return { window, calls, fire, context };
}

test('uncaught errors are posted with the exception chain and source location', () => {
  const { calls, fire, context } = load();
  const error = new context.TypeError('x is undefined');
  fire('error', { error, filename: 'tauri://localhost/js/app.js', lineno: 42, colno: 7 });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].cmd, 'plugin:bugsplat|report_error');
  const { exceptions, attributes } = calls[0].args.report;
  assert.equal(exceptions[0].type, 'TypeError');
  assert.equal(exceptions[0].message, 'x is undefined');
  assert.equal(exceptions[0].id, 0);
  assert.equal(exceptions[0].parentId, null);
  assert.ok(exceptions[0].stack.includes('x is undefined'));
  assert.equal(attributes['js.source'], 'tauri://localhost/js/app.js:42:7');
  assert.equal(attributes.url, 'tauri://localhost/index.html');
});

test('cause and AggregateError.errors are walked depth-first, cause first', () => {
  const { context } = load();
  const root = new context.Error('outer', { cause: new context.Error('inner') });
  root.errors = [new context.Error('agg0')];
  const chain = context.window.__BUGSPLAT__.chain(root);
  assert.deepEqual(JSON.parse(JSON.stringify(chain.map(e => [e.id, e.parentId, e.source, e.message]))), [
    [0, null, null, 'outer'],
    [1, 0, 'cause', 'inner'],
    [2, 0, 'errors[0]', 'agg0'],
  ]);
});

test('cycles are not followed', () => {
  const { context } = load();
  const a = new context.Error('a');
  const b = new context.Error('b', { cause: a });
  a.cause = b;
  assert.equal(context.window.__BUGSPLAT__.chain(a).length, 2);
});

test('non-Error rejections and custom properties are described', () => {
  const { calls, fire, context } = load();
  fire('unhandledrejection', { reason: 'plain string' });
  const err = new context.Error('with props');
  err.code = 'E_FAIL';
  fire('unhandledrejection', { reason: err });
  assert.equal(calls[0].args.report.exceptions[0].type, 'string');
  assert.equal(calls[0].args.report.exceptions[0].message, 'plain string');
  assert.equal(calls[0].args.report.attributes['js.kind'], 'unhandledrejection');
  assert.deepEqual({ ...calls[1].args.report.exceptions[0].properties }, { code: 'E_FAIL' });
});

test('error events without an Error object still report', () => {
  const { calls, fire } = load();
  fire('error', { error: null, message: 'Script error.', filename: '', lineno: 0, colno: 0 });
  assert.equal(calls[0].args.report.exceptions[0].message, 'Script error.');
});

test('reports are capped per page', () => {
  const { calls, fire, context } = load();
  for (let i = 0; i < 30; i++) fire('error', { error: new context.Error('e' + i), filename: '', lineno: 0, colno: 0 });
  assert.equal(calls.length, 20);
});

test('manual posts carry custom attributes', async () => {
  const { calls, window, context } = load();
  await window.BugSplat.post(new context.Error('manual'), { feature: 'export', n: 3 });
  assert.equal(calls[0].args.report.attributes.feature, 'export');
  assert.equal(calls[0].args.report.attributes.n, '3');
});

test('the script installs once', () => {
  const { context, window } = load();
  vm.runInContext(source, context);
  assert.ok(window.BugSplat);
});
