// Thin wrapper over the Tauri bridge. Tests replace window.__TAURI__ with a fake before this loads.
(function () {
  const T = window.__TAURI__;
  const invoke = (cmd, args) => T.core.invoke(cmd, args);
  window.api = {
    defaults: () => invoke('defaults'),
    roots: () => invoke('roots'),
    listDir: (path) => invoke('list_dir', { path }),
    preview: (req) => invoke('preview', { req }),
    views: async (req) => new Uint8Array(await invoke('views', { req })),
    startJob: (job) => invoke('start_job', { job }),
    textEstimate: (req) => invoke('text_estimate', { req }),
    onJob: (cb) => T.event.listen('job', (e) => cb(e.payload)),
    pickFolder: async () => {
      const r = await T.dialog.open({ directory: true, multiple: false });
      return typeof r === 'string' ? r : null;
    },
  };
})();
