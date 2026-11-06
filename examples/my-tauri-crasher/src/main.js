// tauri-plugin-bugsplat reports uncaught errors and unhandled rejections from this page
// automatically; window.BugSplat.post reports errors you catch yourself.
const { invoke } = window.__TAURI__.core;
const { openUrl } = window.__TAURI__.opener;

function loadUserSettings() {
    throw new Error('Could not load user settings', { cause: new Error('settings.json is missing') });
}

window.addEventListener('DOMContentLoaded', () => {
    // Native crash and Rust panic: Tauri commands in src-tauri/src/lib.rs.
    document.getElementById('native-crash-button').onclick = () => invoke('native_crash');
    document.getElementById('rust-panic-button').onclick = () => invoke('rust_panic');

    document.getElementById('js-error-button').onclick = () => {
        loadUserSettings();
    };

    document.getElementById('js-rejection-button').onclick = () => {
        Promise.reject(new Error('Rejected promise that nobody awaited'));
    };

    document.getElementById('js-post-button').onclick = async () => {
        try {
            loadUserSettings();
        } catch (error) {
            await window.BugSplat.post(error, { screen: 'settings' });
        }
    };

    // Open links in the browser rather than navigating this window away.
    for (const link of document.querySelectorAll('a.external')) {
        link.addEventListener('click', (event) => {
            event.preventDefault();
            openUrl(link.href);
        });
    }
});
