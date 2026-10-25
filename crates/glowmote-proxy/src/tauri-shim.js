// Stands in for Tauri's bridge so the desktop app's UI runs in a browser against glowmote-proxy.
window.__TAURI__ = {
  core: {
    async invoke(command, args = {}) {
      const res = await fetch(`/api/${command}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(args),
      });
      const body = await res.json().catch(() => ({ error: `${res.status} ${res.statusText}` }));
      if (!res.ok) throw body.error;
      return body.ok;
    },
  },
  // No server-pushed events; listeners never fire.
  event: { async listen() { return () => {}; } },
  proxy: true,
};
