const API_BASE = window.location.origin;
let latestWalletStatus = null;

const MAIN_META = {
    "logging.level": {
        label: "TRYB LOGOWANIA",
        type: "select",
        options: ["trace", "debug", "info", "warn", "error"]
    },
    "trigger.jito_tip.base_tip_percent": { label: "BAZOWY JITO TIP [%]" },
    "trigger.jito_tip.dynamic_tip_percent": { label: "DYNAMICZNY JITO TIP [%]" },
    "trigger.jito_tip.max_tip_percent": { label: "MAX. JITO TIP [%]" },
    "trigger.jito_tip.max_tip_absolute_sol": { label: "MAX. JITO TIP W SOL" },
    "trigger.jito_tip.max_tip_ratio_percent": { label: "MAX. JITO TIP RATIO [%]" },
    "trigger.jito_tip.fallback_tip_sol": { label: "FALLBACK JITO TIP W SOL" },
    "seer.source_mode": {
        label: "TRYB ŹRÓDŁA SEER",
        type: "select",
        options: ["pump_portal_ws", "grpc", "helius_websocket"]
    },
    "seer.enable_pumpfun": { label: "SEER PUMPFUN AKTYWNY" },
    "seer.enable_raydium": { label: "SEER RAYDIUM AKTYWNY" },
    "trigger.max_position_size_sol": { label: "KWOTA SOL NA TRANSAKCJĘ" },
    "trigger.slippage_tolerance": { label: "MAKS. SLIPPAGE" },
    "trigger.wallet_address": { label: "ADRES PORTFELA SOLANA WALLET:", readOnly: true, excludeFromSave: true, gridClass: "field-span-2" },
    "trigger.max_concurrent_positions": { label: "MAKS. RÓWNOLEGŁE POZYCJE" },
    "trigger.dry_run": { label: "TRYB DRY RUN" }
};

const SECTION_LABEL_OVERRIDES = {
    gatekeeper_v2: {
        dev_unknown_max_single_tx_price_impact_pc: "dev_unknown_max_single_tx_price_ipct"
    }
};

document.addEventListener("DOMContentLoaded", () => {
    setConnectionChip(false);
    bindActions();
    bindTabs();
    bindAppearance();
    restoreAppearance();
    refreshAll();
    setInterval(() => {
        refreshRuntimeStatus();
        refreshWalletStatus();
    }, 5000);
});

function bindActions() {
    document.getElementById("startRunBtn").addEventListener("click", startRun);
    document.getElementById("stopRunBtn").addEventListener("click", stopRun);
    document.getElementById("saveConfigBtn").addEventListener("click", saveConfig);
    document.getElementById("refreshStatusBtn").addEventListener("click", refreshRuntimeStatus);

    document.getElementById("modeGrpc").addEventListener("change", onConnectionModeChange);
    document.getElementById("modePumpPortal").addEventListener("change", onConnectionModeChange);
}

function bindTabs() {
    document.querySelectorAll(".tab-btn").forEach((btn) => {
        btn.addEventListener("click", () => {
            document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
            document.querySelectorAll(".tab-content").forEach((c) => c.classList.remove("active"));
            btn.classList.add("active");
            document.getElementById(btn.dataset.tab).classList.add("active");
        });
    });
}

/* ---------- Appearance (background & banner) ---------- */

function bindAppearance() {
    document.getElementById("bannerClearBtn").addEventListener("click", clearBanner);
    document.getElementById("bannerFileInput").addEventListener("change", (e) => {
        const file = e.target.files[0];
        if (!file) return;
        const reader = new FileReader();
        reader.onload = (ev) => {
            applyBanner(ev.target.result);
            localStorage.setItem("ghost_banner_image", ev.target.result);
        };
        reader.readAsDataURL(file);
    });

    document.getElementById("bgApplyBtn").addEventListener("click", () => {
        const url = document.getElementById("bgImageUrl").value.trim();
        if (url) {
            applyBackground(url);
            localStorage.setItem("ghost_bg_image", url);
        }
    });
    document.getElementById("bgClearBtn").addEventListener("click", () => {
        clearBackground();
        localStorage.removeItem("ghost_bg_image");
        localStorage.removeItem("ghost_bg_file");
        document.getElementById("bgImageUrl").value = "";
    });
    document.getElementById("bgFileInput").addEventListener("change", (e) => {
        const file = e.target.files[0];
        if (!file) return;
        const reader = new FileReader();
        reader.onload = (ev) => {
            applyBackground(ev.target.result);
            localStorage.setItem("ghost_bg_file", ev.target.result);
            localStorage.removeItem("ghost_bg_image");
        };
        reader.readAsDataURL(file);
    });
}

function restoreAppearance() {
    const bannerImage = localStorage.getItem("ghost_banner_image");
    const bgUrl = localStorage.getItem("ghost_bg_image");
    const bgFile = localStorage.getItem("ghost_bg_file");

    if (bannerImage) {
        applyBanner(bannerImage);
    }

    if (bgUrl) {
        applyBackground(bgUrl);
        document.getElementById("bgImageUrl").value = bgUrl;
    } else if (bgFile) {
        applyBackground(bgFile);
    }
}

function applyBackground(src) {
    document.body.style.backgroundImage = "url('" + src + "')";
}

function clearBackground() {
    document.body.style.backgroundImage = "";
}

function applyBanner(src) {
    const bannerSlot = document.getElementById("bannerSlot");
    bannerSlot.style.backgroundImage = `linear-gradient(180deg, rgba(4, 9, 18, 0.18) 0%, rgba(4, 9, 18, 0.28) 100%), url('${src}')`;
    bannerSlot.classList.add("has-image");
}

function clearBanner() {
    const bannerSlot = document.getElementById("bannerSlot");
    const bannerInput = document.getElementById("bannerFileInput");
    bannerSlot.style.backgroundImage = "";
    bannerSlot.classList.remove("has-image");
    bannerInput.value = "";
    localStorage.removeItem("ghost_banner_image");
}

/* ---------- Connection mode checkboxes ---------- */

function onConnectionModeChange(e) {
    const grpcBox = document.getElementById("modeGrpc");
    const ppBox = document.getElementById("modePumpPortal");

    if (e.target === grpcBox && grpcBox.checked) {
        ppBox.checked = false;
    } else if (e.target === ppBox && ppBox.checked) {
        grpcBox.checked = false;
    }

    syncConnectionModeToConfig();
}

function syncConnectionModeToConfig() {
    const grpcBox = document.getElementById("modeGrpc");
    const ppBox = document.getElementById("modePumpPortal");

    let mode = "pump_portal_ws";
    if (grpcBox.checked) mode = "grpc";
    else if (ppBox.checked) mode = "pump_portal_ws";

    const select = document.querySelector('[data-key="seer.source_mode"]');
    if (select) select.value = mode;
}

function syncConnectionModeFromConfig(sourceMode) {
    const grpcBox = document.getElementById("modeGrpc");
    const ppBox = document.getElementById("modePumpPortal");
    grpcBox.checked = sourceMode === "grpc";
    ppBox.checked = sourceMode === "pump_portal_ws";
}

/* ---------- Refresh & control ---------- */

async function refreshAll() {
    await Promise.all([refreshRuntimeStatus(), loadSystemConfig(), refreshWalletStatus()]);
}

async function refreshRuntimeStatus() {
    try {
        const [statusRes, controlRes] = await Promise.all([
            fetch(`${API_BASE}/status`),
            fetch(`${API_BASE}/control/status`)
        ]);

        const status = await statusRes.json();
        const control = await controlRes.json();

        document.getElementById("systemMode").textContent = status.mode;
        document.getElementById("tmuxStatus").textContent = control.tmux_session_exists ? "ACTIVE" : "INACTIVE";
        document.getElementById("launcherStatus").textContent = control.launcher_process_running ? "RUNNING" : "STOPPED";
        setConnectionChip(true);
    } catch (error) {
        setConnectionChip(false);
        showMessage("controlMessage", false, `Błąd statusu: ${error.message}`);
    }
}

async function refreshWalletStatus() {
    try {
        const resp = await fetch(`${API_BASE}/wallet/status`);
        const data = await resp.json();
        if (!resp.ok) {
            throw new Error(data.error || "Nie udało się pobrać statusu portfela");
        }

        latestWalletStatus = data;
        applyWalletStatusToUi();
    } catch (_error) {
        latestWalletStatus = null;
        applyWalletStatusToUi();
    }
}

function applyWalletStatusToUi() {
    const addressLine = document.getElementById("walletAddressLine");
    const balanceLine = document.getElementById("walletBalanceLine");
    const walletAddressField = document.getElementById("walletAddressField");

    const walletAddress = latestWalletStatus?.wallet_address || "";
    const walletBalance = latestWalletStatus?.sol_balance_sol != null
        ? formatSolBalance(latestWalletStatus.sol_balance_sol)
        : "";

    addressLine.textContent = `Portfel: ${walletAddress}`;
    balanceLine.textContent = `Wartość portfela w SOL: ${walletBalance}`;

    if (walletAddressField) {
        walletAddressField.value = walletAddress;
    }
}

function formatSolBalance(value) {
    return Number(value).toLocaleString("pl-PL", {
        minimumFractionDigits: 0,
        maximumFractionDigits: 6
    });
}

function setConnectionChip(isConnected) {
    const chip = document.getElementById("connectionChip");
    chip.textContent = isConnected ? "Connected" : "Disconnected";
    chip.classList.toggle("is-connected", isConnected);
    chip.classList.toggle("is-disconnected", !isConnected);
}

async function startRun() {
    try {
        const resp = await fetch(`${API_BASE}/control/start`, { method: "POST" });
        const data = await resp.json();
        if (!resp.ok || !data.success) {
            throw new Error(data.error || data.message || "Nie udało się uruchomić");
        }
        showMessage("controlMessage", true, data.message);
        await refreshRuntimeStatus();
    } catch (error) {
        showMessage("controlMessage", false, `START: ${error.message}`);
    }
}

async function stopRun() {
    try {
        const resp = await fetch(`${API_BASE}/control/stop-run`, { method: "POST" });
        const data = await resp.json();
        if (!resp.ok || !data.success) {
            throw new Error(data.error || data.message || "Nie udało się zatrzymać");
        }
        showMessage("controlMessage", true, data.message);
        await refreshRuntimeStatus();
    } catch (error) {
        showMessage("controlMessage", false, `STOP: ${error.message}`);
    }
}

async function loadSystemConfig() {
    try {
        const resp = await fetch(`${API_BASE}/config/system`);
        const data = await resp.json();
        if (!resp.ok) {
            throw new Error(data.error || "Nie udało się wczytać konfiguracji");
        }

        renderMainConfig(data.main || {});
        renderSection("gatekeeperGrid", "gatekeeper_v2", data.gatekeeper_v2 || {});
        renderSection("iwimGrid", "iwim", data.iwim || {});
        renderSection("iwimVetoGrid", "iwim_veto_gate", data.iwim_veto_gate || {});

        // Sync connection mode checkboxes
        if (data.main && data.main["seer.source_mode"] != null) {
            syncConnectionModeFromConfig(String(data.main["seer.source_mode"]));
        }
    } catch (error) {
        showMessage("configMessage", false, `Błąd odczytu configu: ${error.message}`);
    }
}

function renderMainConfig(values) {
    const root = document.getElementById("mainConfigGrid");
    root.innerHTML = "";

    Object.keys(MAIN_META).forEach((key) => {
        if (MAIN_META[key]?.excludeFromSave) {
            root.appendChild(createStaticField(MAIN_META[key].label, latestWalletStatus?.wallet_address || "", MAIN_META[key]));
            return;
        }

        root.appendChild(createField("main", key, values[key], MAIN_META[key]));
    });
}

function renderSection(containerId, sectionName, values) {
    const root = document.getElementById(containerId);
    root.innerHTML = "";

    Object.keys(values)
        .sort()
        .forEach((key) => {
            root.appendChild(createField(sectionName, key, values[key], null));
        });
}

function createField(section, key, value, meta) {
    const wrapper = document.createElement("div");
    wrapper.className = "field";
    if (meta?.gridClass) {
        wrapper.classList.add(meta.gridClass);
    }
    if (section === "gatekeeper_v2") {
        wrapper.classList.add("parametric-field");
    }

    const label = document.createElement("label");
    label.textContent = getDisplayLabel(section, key, meta);
    if (meta?.label) {
        label.classList.add("friendly-label");
    }
    wrapper.appendChild(label);

    let input;
    const valueType = inferType(value);

    if (meta?.type === "select") {
        input = document.createElement("select");
        (meta.options || []).forEach((option) => {
            const opt = document.createElement("option");
            opt.value = option;
            opt.textContent = option;
            input.appendChild(opt);
        });
        input.value = String(value ?? "");
    } else if (valueType === "boolean") {
        const checkWrap = document.createElement("div");
        checkWrap.className = "check-wrap";
        input = document.createElement("input");
        input.type = "checkbox";
        input.checked = Boolean(value);
        const txt = document.createElement("span");
        txt.textContent = "enabled";
        checkWrap.appendChild(input);
        checkWrap.appendChild(txt);
        wrapper.appendChild(checkWrap);
    } else {
        input = document.createElement("input");
        input.type = valueType === "number" ? "number" : "text";
        if (valueType === "number") {
            input.step = "any";
        }
        input.value = value ?? "";
    }

    if (!wrapper.querySelector(".check-wrap")) {
        wrapper.appendChild(input);
    }

    input.dataset.section = section;
    input.dataset.key = key;
    input.dataset.valueType = valueType;
    return wrapper;
}

function createStaticField(labelText, value, meta = {}) {
    const wrapper = document.createElement("div");
    wrapper.className = "field";
    if (meta.gridClass) {
        wrapper.classList.add(meta.gridClass);
    }

    const label = document.createElement("label");
    label.textContent = labelText;
    label.classList.add("friendly-label");
    wrapper.appendChild(label);

    const input = document.createElement("input");
    input.type = "text";
    input.value = value || "";
    input.readOnly = true;
    input.id = "walletAddressField";
    wrapper.appendChild(input);

    return wrapper;
}

function getDisplayLabel(section, key, meta) {
    if (meta?.label) {
        return meta.label;
    }

    const sectionOverrides = SECTION_LABEL_OVERRIDES[section] || {};
    return sectionOverrides[key] || key;
}

function inferType(value) {
    if (typeof value === "boolean") return "boolean";
    if (typeof value === "number") return "number";
    return "string";
}

async function saveConfig() {
    try {
        const payload = {
            main: collectSectionValues("main"),
            gatekeeper_v2: collectSectionValues("gatekeeper_v2"),
            iwim: collectSectionValues("iwim"),
            iwim_veto_gate: collectSectionValues("iwim_veto_gate")
        };

        const resp = await fetch(`${API_BASE}/config/system`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload)
        });
        const data = await resp.json();
        if (!resp.ok) {
            throw new Error(data.error || "Nie udało się zapisać configu");
        }

        showMessage("configMessage", true, "Konfiguracja została zapisana.");
        renderMainConfig(data.main || {});
        renderSection("gatekeeperGrid", "gatekeeper_v2", data.gatekeeper_v2 || {});
        renderSection("iwimGrid", "iwim", data.iwim || {});
        renderSection("iwimVetoGrid", "iwim_veto_gate", data.iwim_veto_gate || {});

        if (data.main && data.main["seer.source_mode"] != null) {
            syncConnectionModeFromConfig(String(data.main["seer.source_mode"]));
        }
    } catch (error) {
        showMessage("configMessage", false, `Zapis nieudany: ${error.message}`);
    }
}

function collectSectionValues(section) {
    const fields = document.querySelectorAll(`[data-section="${section}"]`);
    const result = {};

    fields.forEach((field) => {
        const key = field.dataset.key;
        const valueType = field.dataset.valueType;
        let value;

        if (valueType === "boolean") {
            value = Boolean(field.checked);
        } else if (valueType === "number") {
            value = parseNumberPreserveInt(field.value);
        } else {
            value = field.value;
        }

        result[key] = value;
    });

    return result;
}

function parseNumberPreserveInt(raw) {
    const parsed = Number(raw);
    if (Number.isNaN(parsed)) {
        return 0;
    }
    if (Number.isInteger(parsed)) {
        return parsed;
    }
    return parsed;
}

function showMessage(containerId, success, message) {
    const box = document.getElementById(containerId);
    box.textContent = message;
    box.className = `message show ${success ? "success" : "error"}`;
    setTimeout(() => {
        box.classList.remove("show");
    }, 4500);
}
