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
		const tauri = window.__TAURI__;

		if (!tauri) {
			console.log("DECORUM: Tauri API not found. Exiting.");
			console.log(
				"DECORUM: Set withGlobalTauri: true in tauri.conf.json to enable.",
			);
			return;
		}

		const win = tauri.window.getCurrentWindow();

		console.log("DECORUM: Waiting for [data-tauri-decorum-tb] ...");

		// Add debounce function
		const debounce = (func, delay) => {
			let timeoutId;
			return (...args) => {
				clearTimeout(timeoutId);
				timeoutId = setTimeout(() => func(...args), delay);
			};
		};

		// Dark/light mode aware hover colors (fixes #39)
		const BUTTON_HOVER_BG_LIGHT = "rgba(0,0,0,0.1)";
		const BUTTON_HOVER_BG_DARK = "rgba(255,255,255,0.1)";
		const CLOSE_HOVER_BG = "rgba(255,0,0,0.7)";

		const getButtonHoverBg = () =>
			window.matchMedia("(prefers-color-scheme: dark)").matches
				? BUTTON_HOVER_BG_DARK
				: BUTTON_HOVER_BG_LIGHT;

		// Track active control for hover rendering
		let activeControl = null;
		const buttons = new Map();

		const renderHover = () => {
			const hoverBg = getButtonHoverBg();
			buttons.forEach(({ button, isClose }, control) => {
				button.style.backgroundColor =
					control === activeControl
						? isClose
							? CLOSE_HOVER_BG
							: hoverBg
						: "transparent";
			});
		};

		const setActiveControl = (control) => {
			activeControl = control;
			renderHover();
		};

		// Hit-test using elementFromPoint to determine which button is hovered.
		// Called from the native snap overlay's mousemove event so that hover
		// states stay in sync when the invisible Win32 child HWND intercepts
		// pointer events over the maximize button.
		const hitTestControls = (x, y) => {
			const element = document.elementFromPoint(x, y);
			const button = element?.closest?.("[id^='decorum-tb-']");
			setActiveControl(
				button ? button.id.slice("decorum-tb-".length) : null,
			);
		};

		// Re-render hover when color scheme changes
		window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
			renderHover();
		});

		// Create button func
		const createButton = (id) => {
			const btn = document.createElement("button");

			btn.id = "decorum-tb-" + id;
			btn.style.width = "58px";
			btn.style.height = "32px";
			btn.style.border = "none";
			btn.style.padding = "0px";
			btn.style.outline = "none";
			btn.style.display = "flex";
			btn.style.fontSize = "10px";
			btn.style.fontWeight = "300";
			btn.style.cursor = "default";
			btn.style.boxShadow = "none";
			btn.style.borderRadius = "0px";
			btn.style.alignItems = "center";
			btn.style.justifyContent = "center";
			btn.style.transition = "background 0.1s";
			btn.style.backgroundColor = "transparent";
			btn.style.textRendering = "optimizeLegibility";
			btn.style.fontFamily = "'Segoe Fluent Icons', 'Segoe MDL2 Assets'";

			const isClose = id === "close";

			const setHover = (hovered) => {
				if (hovered) {
					setActiveControl(id);
				} else if (activeControl === id) {
					setActiveControl(null);
				}
			};

			const state = {
				actionLock: false,
				lastAction: 0,
			};

			const tryAction = (action) => {
				const now = Date.now();
				if (state.actionLock || now - state.lastAction < 200) return;
				state.actionLock = true;
				state.lastAction = now;
				setHover(false);
				Promise.resolve(action()).finally(() => {
					setTimeout(() => { state.actionLock = false; }, 100);
				});
			};

			buttons.set(id, { button: btn, isClose });

			btn.onmouseenter = () => setHover(true);
			btn.onmouseleave = () => setHover(false);

			switch (id) {
				case "minimize":
					btn.innerHTML = "\uE921";
					btn.setAttribute("aria-label", "Minimize window");
					btn.onclick = (e) => {
						e.preventDefault();
						tryAction(() => win.minimize());
					};
					break;
				case "maximize":
					btn.innerHTML = "\uE922";
					btn.setAttribute("aria-label", "Maximize window");

					const toggleMaximize = (e) => {
						if (e) e.preventDefault();
						tryAction(() => win.toggleMaximize());
					};
					btn.onclick = toggleMaximize;

					// Listen for native snap overlay events emitted by the
					// Rust snap module's child HWND.  These keep hover state
					// in sync and forward clicks so that the native Windows
					// 11 Snap Layout flyout works without keyboard simulation.
					win.listen("decorum://snap/mousemove", ({ payload }) => {
						if (Array.isArray(payload)) hitTestControls(payload[0], payload[1]);
					});
					win.listen("decorum://snap/mouseenter", () => setHover(true));
					win.listen("decorum://snap/mouseleave", () => setHover(false));
					win.listen("decorum://snap/mousedown", () => setHover(true));
					win.listen("decorum://snap/mouseup", () => setHover(true));
					win.listen("decorum://snap/click", () => toggleMaximize());

					win.onResized(() => {
						win.isMaximized().then((maximized) => {
							if (maximized) {
								btn.innerHTML = "\uE923";
								btn.setAttribute("aria-label", "Restore window size");
							} else {
								btn.innerHTML = "\uE922";
								btn.setAttribute("aria-label", "Maximize window size");
							}
						});
					});
					break;
				case "close":
					btn.innerHTML = "\uE8BB";
					btn.setAttribute("aria-label", "Close window");
					btn.onclick = () => win.close();
					break;
			}

			return btn;
		};

		// Debounce the control creation
		const debouncedCreateControls = debounce(() => {
			const tbEl = document.querySelector("[data-tauri-decorum-tb]");
			if (!tbEl) return;

			// Check if controls already exist
			if (tbEl.querySelector("[id^='decorum-tb-']")) {
				console.log("DECORUM: Controls already exist. Skipping creation.");
				return;
			}

			// Before eval-ing, the line below is modified from the rust side
			// to only include the controls that are enabled on the window
			["minimize", "maximize", "close"].forEach((id) => {
				const btn = createButton(id);
				tbEl.appendChild(btn);
			});
		});

		// Use MutationObserver to watch for changes
		const observer = new MutationObserver((mutations) => {
			for (let mutation of mutations) {
				if (mutation.type === "childList") {
					const tbEl = document.querySelector("[data-tauri-decorum-tb]");
					if (tbEl) {
						debouncedCreateControls();
						break;
					}
				}
			}
		});

		// data-tauri-decorum-tb may be created before observer starts
		if (document.querySelector("[data-tauri-decorum-tb]")) {
			debouncedCreateControls();
			return;
		}

		observer.observe(document.body, {
			childList: true,
			subtree: true,
		});

		debouncedCreateControls();
	};

	// Fix for #50/#32: scripts may be injected after DOMContentLoaded
	// has already fired, so check readyState instead of always waiting.
	if (document.readyState === "loading") {
		document.addEventListener("DOMContentLoaded", setup);
	} else {
		setup();
	}
})();
