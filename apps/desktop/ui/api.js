// Thin wrapper over the Tauri bridge. Tests replace window.__TAURI__ with a fake before this loads.
(function () {
  const T = window.__TAURI__;
  const invoke = (cmd, args) => T.core.invoke(cmd, args);
  window.api = {
    defaults: () => invoke('defaults'),
    roots: () => invoke('roots'),
    listDir: (path) => invoke('list_dir', { path }),
    describe: (paths, accept, recursive) => invoke('describe', { paths, accept, recursive: !!recursive }),
    saveText: (path, text) => invoke('save_text', { path, text }),
    preview: (req) => invoke('preview', { req }),
    views: async (req) => new Uint8Array(await invoke('views', { req })),
    startJob: (job) => invoke('start_job', { job }),
    textEstimate: (req) => invoke('text_estimate', { req }),
    onJob: (cb) => T.event.listen('job', (e) => cb(e.payload)),
    // Native file windows. Each returns an array of paths ([] when cancelled).
    pickFiles: async (title, filters) => {
      const r = await T.dialog.open({ title, multiple: true, directory: false, filters });
      return Array.isArray(r) ? r : typeof r === 'string' ? [r] : [];
    },
    pickFolders: async (title) => {
      const r = await T.dialog.open({ title, multiple: true, directory: true });
      return Array.isArray(r) ? r : typeof r === 'string' ? [r] : [];
    },
    saveDialog: async (defaultPath) => {
      const r = await T.dialog.save({ defaultPath, filters: [{ name: 'Text', extensions: ['txt'] }] });
      return typeof r === 'string' ? r : null;
    },
    pickFolder: async () => {
      const r = await T.dialog.open({ directory: true, multiple: false });
      return typeof r === 'string' ? r : null;
    },
  };
})();
