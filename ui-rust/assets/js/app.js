// parasanoid Rust UI — small progressive-enhancement behaviours.
// Loaded as a classic script (Bootstrap is a module).

(function () {
  "use strict";

  const SHELL = "appShell";
  const STORAGE_KEY = "parasanoid-sidebar";
  const shell = document.getElementById(SHELL);
  const toggle = document.getElementById("sidebarCollapse");

  function setCollapsed(collapsed) {
    if (!shell) return;
    shell.classList.toggle("collapsed", collapsed);
    if (toggle) {
      toggle.setAttribute("aria-expanded", String(!collapsed));
    }
  }

  // Restore a persisted preference, then allow the user to toggle it.
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "collapsed" || saved === "expanded") {
      setCollapsed(saved === "collapsed");
    }
  } catch (_e) {
    /* ignore storage access errors (private mode, etc.) */
  }

  if (toggle && shell) {
    toggle.addEventListener("click", function () {
      const collapsed = shell.classList.toggle("collapsed");
      try {
        localStorage.setItem(STORAGE_KEY, collapsed ? "collapsed" : "expanded");
      } catch (_e) {
        /* storage may be unavailable */
      }
      toggle.setAttribute("aria-expanded", String(!collapsed));
    });
  }
})();
