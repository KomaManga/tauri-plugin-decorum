/**
 * @param {string} selector
 * @returns {Promise<HTMLElement>}
 */
function waitForElm(selector) {
  return new Promise((resolve) => {
    if (document.querySelector(selector)) {
      return resolve(document.querySelector(selector));
    }

    const observer = new MutationObserver((mutations) => {
      if (document.querySelector(selector)) {
        observer.disconnect();
        resolve(document.querySelector(selector));
      }
    });

    // If you get "parameter 1 is not of type 'Node'" error, see https://stackoverflow.com/a/77855838/492336
    observer.observe(document.body, {
      childList: true,
      subtree: true,
    });
  });
}

(() => {
  const setup = () => {
  // All of this tags will be replaced by the found system theme icons
  const windowCloseSvg = `@win-close`;
  const windowMinimizeSvg = `@win-minimize`;
  const windowMaximizeSvg = `@win-maximize`;
  const windowRestoreSvg = `@win-restore`;

  const tauri = window.__TAURI__;

  if (!tauri) {
    console.log("DECORATION: Tauri API not found. Exiting.");
    console.log(
      "DECORATION: Set withGlobalTauri: true in tauri.conf.json to enable.",
    );
    return;
  }

  const win = tauri.window.getCurrentWindow();

  console.log("DECORATION: Waiting for [data-tauri-decoration-tb] ...");

  const ensureControlHost = () => {
    let controlsEl = document.querySelector("[data-tauri-decoration-controls]");
    if (controlsEl) return controlsEl;

    controlsEl = document.createElement("div");
    controlsEl.setAttribute("data-tauri-decoration-controls", "");
    controlsEl.setAttribute("role", "group");
    controlsEl.setAttribute("lang", "en");
    controlsEl.setAttribute("aria-label", "Window controls");
    controlsEl.style.top = "0";
    controlsEl.style.right = "0";
    controlsEl.style.zIndex = "300";
    controlsEl.style.height = "32px";
    controlsEl.style.display = "flex";
    controlsEl.style.position = "fixed";
    controlsEl.style.alignItems = "center";
    controlsEl.style.justifyContent = "end";
    controlsEl.style.backgroundColor = "transparent";
    document.body.appendChild(controlsEl);
    return controlsEl;
  };

  waitForElm("[data-tauri-decoration-tb]").then((tbEl) => {
    const controlsEl = ensureControlHost();
    if (controlsEl.querySelector("[id^='decoration-tb-']")) {
      console.log("DECORATION: Controls already exist. Skipping creation.");
      return;
    }

    const actions = document.createElement("div");
    actions.className = "decoration-tb-actions";
    actions.style.width = "fit-content";
    actions.style.display = "flex";
    actions.style.paddingRight = "0.5em";
    actions.style.gap = "0.8125em";

    // Create button func
    const createButton = (id) => {
      console.debug("createButton", id);
      const btn = document.createElement("button");
      btn.id = "decoration-tb-" + id;
      btn.classList.add("decoration-tb-btn");

      switch (id) {
        case "minimize":
          btn.innerHTML = windowMinimizeSvg;

          btn.addEventListener("click", () => {
            win.minimize();
          });

          break;
        case "maximize":
          btn.innerHTML = windowMaximizeSvg;
          win.onResized(() => {
            win.isMaximized().then((maximized) => {
              if (maximized) {
                btn.innerHTML = windowRestoreSvg;
              } else {
                btn.innerHTML = windowMaximizeSvg;
              }
            });
          });

          btn.addEventListener("click", () => {
            btn.blur();
            win.toggleMaximize();
          });

          break;
        case "close":
          btn.innerHTML = windowCloseSvg;
          btn.addEventListener("click", () => win.close());
          break;
      }

      actions.appendChild(btn);
    };

    // Before eval-ing, the line below is modified from the rust side
    // to only include the controls that are enabled on the window
    ["minimize", "maximize", "close"].forEach(createButton);

    controlsEl.appendChild(actions);

    const style = document.createElement("style");
    document.head.appendChild(style);

    style.innerHTML = `
			.decoration-tb-btn {
        color: white;
				cursor: default;
				border: none;
				padding: 0px;
      	width: 1.5em;
				height: 1.5em;
				outline: none;
				display: flex;
				box-shadow: none;
				align-items: center;
				justify-content: center;
				transition: background 0.1s;
      	border-radius: 50%;
				background-color: var(--decoration-tb-actions-icon-bg, rgba(255, 255, 255, 0.2));
			}

      .decoration-tb-btn:hover {
        background-color: var(--decoration-tb-actions-icon-active-bg, rgba(255, 255, 255, 0.4));
      }

      .decoration-tb-btn svg {
      	width: 16px;
				height: 16px;
      }

      .decoration-tb-btn svg path {
        fill: var(--decoration-tb-actions-icon-fg, #ffffff);
      }
		`;
  });
  };

  // Fix for #50/#32: scripts may be injected after DOMContentLoaded
  // has already fired, so check readyState instead of always waiting.
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", setup);
  } else {
    setup();
  }
})();
