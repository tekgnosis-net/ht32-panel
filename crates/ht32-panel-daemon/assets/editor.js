function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)); }

function defaultContent(kind) {
  switch (kind) {
    case "text":      return { kind:"text",      value:{src:"hostname"},   size:12.0, color:"primary",   align:"left" };
    case "bar":       return { kind:"bar",        value:"cpu_percent",     fill:"primary",  bg:2105376 };
    case "gauge":     return { kind:"gauge",      value:"cpu_temp",        min:0.0,   max:100.0, color:"primary", track:"background" };
    case "sparkline": return { kind:"sparkline",  a:"disk_history",        b:null, wrap_around:true,
                               color_a:"primary", color_b:"secondary",     bg:0, scale:"auto" };
    case "clock":     return { kind:"clock",      mode:"digital",          color:"text" };
  }
}

window.editor = function () {
  return {
    schema: { kinds: [], text_sources: [], number_sources: [], theme_slots: [],
              time_fmts: [], date_fmts: [], number_fmts: [] },
    templates: [], name: "",
    spec: { name: "", orientation: "portrait", widgets: [] },
    dims: [170, 320],
    sel: null, truthSrc: "", warnings: [], status: "", scale: 1.5,
    get warnIds() { return this.warnings.map(w => w.widget_id); },
    _t: null,

    async init() {
      this.schema = await (await fetch("/api/template-schema")).json();
      this.templates = await (await fetch("/api/templates")).json();
      this.dims = this.spec.orientation.startsWith("portrait") ? [170,320] : [320,170];
      const q = new URLSearchParams(location.search).get("name");
      if (q) { this.name = q; await this.load(); }
    },

    deviceStyle() {
      const [w,h] = this.dims; const s = this.scale;
      return `width:${w*s}px;height:${h*s}px;background:${this.bgCss()}`;
    },

    widgetStyle(w) {
      const s = this.scale; const r = w.rect;
      return `left:${r.x*s}px;top:${r.y*s}px;width:${r.w*s}px;height:${r.h*s}px;`+
             `font-size:10px;color:#888;background:#222`;
    },

    select(i) { this.sel = i; },

    startDrag(i, ev, mode) {
      ev.preventDefault();
      this.sel = i;
      const w = this.spec.widgets[i];
      const mouse = { x: ev.clientX, y: ev.clientY };
      const rect = { x: w.rect.x, y: w.rect.y, w: w.rect.w, h: w.rect.h };
      const [dw, dh] = this.dims;
      const onMove = (e) => {
        const dx = Math.round((e.clientX - mouse.x) / this.scale / 2) * 2;
        const dy = Math.round((e.clientY - mouse.y) / this.scale / 2) * 2;
        if (mode === "move") {
          w.rect.x = clamp(rect.x + dx, 0, dw - w.rect.w);
          w.rect.y = clamp(rect.y + dy, 0, dh - w.rect.h);
        } else {
          w.rect.w = clamp(rect.w + dx, 4, dw - w.rect.x);
          w.rect.h = clamp(rect.h + dy, 4, dh - w.rect.y);
        }
        this.renderAll();
      };
      const onUp = () => {
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        this.refreshTruth();
        this.renderAll();
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    },

    renderAll() {
      this.$nextTick(() => {
        const els = document.querySelectorAll(".device .widget");
        els.forEach((el, i) => {
          if (this.spec.widgets[i]) window.renderWidget(el, this.spec.widgets[i]);
        });
      });
    },

    addWidget(kind) {
      const id = kind + "_" + (this.spec.widgets.length + 1);
      this.spec.widgets.push({ id, rect:{x:8,y:8,w:80,h:20}, ...defaultContent(kind) });
      this.sel = this.spec.widgets.length - 1;
      this.refreshTruth();
      this.renderAll();
    },

    async load() {
      if (!this.name) {
        this.spec = { name:"", orientation:"portrait", widgets:[] };
        this.sel = null;
        return;
      }
      this.spec = await (await fetch(`/api/templates/${this.name}`)).json();
      if (!this.spec.orientation) this.spec.orientation = "portrait";
      this.dims = this.spec.orientation.startsWith("portrait") ? [170,320] : [320,170];
      this.sel = null; this.refreshTruth(); this.renderAll();
    },

    async save() {
      this.spec.name = this.name;
      const exists = this.templates.includes(this.name);
      const url = exists ? `/api/templates/${this.name}` : "/api/templates";
      const method = exists ? "PUT" : "POST";
      const resp = await fetch(url, { method, headers:{ "content-type":"application/json" },
                                      body: JSON.stringify(this.spec) });
      if (resp.ok) {
        this.status = "saved";
        this.templates = await (await fetch("/api/templates")).json();
      } else {
        this.status = "error: " + (await resp.text());
      }
    },

    async activate() {
      await this.save();
      const body = new URLSearchParams({ face: this.name });
      await fetch("/face", { method:"POST", body });
      this.status = "activated on panel";
    },

    refreshTruth() {
      clearTimeout(this._t);
      this._t = setTimeout(async () => {
        const resp = await fetch("/api/templates/preview",
          { method:"POST", headers:{ "content-type":"application/json" }, body: JSON.stringify(this.spec) });
        if (!resp.ok) { this.status = "preview error"; return; }
        const j = await resp.json();
        this.truthSrc = "data:image/png;base64," + j.png_base64;
        this.warnings = j.warnings;
      }, 400);
    },

    // ── Colour helpers ───────────────────────────────────────────────────────

    // The currently-selected widget, or null. All property-panel bindings go
    // through this so they are null-safe: Alpine re-evaluates a guarded subtree's
    // child bindings one last time when `sel` flips to null (before x-if tears
    // them down), so the expressions themselves MUST tolerate sel===null.
    get cur() {
      return this.sel === null ? null : (this.spec.widgets[this.sel] ?? null);
    },

    // ── Colour helpers (all null-safe via `cur`) ─────────────────────────────

    colorIsCustom(field) {
      return typeof this.cur?.[field] === "number";
    },

    colorHex(field) {
      const v = this.cur?.[field];
      if (typeof v === "number") return "#" + v.toString(16).padStart(6, "0");
      return "#7aa2f7";
    },

    setColorSlot(field, v) {
      if (!this.cur) return;
      if (v === "custom") {
        this.cur[field] = parseInt(this.colorHex(field).slice(1), 16);
      } else {
        this.cur[field] = v;
      }
      this.refreshTruth();
      this.renderAll();
    },

    setColorHex(field, hex) {
      if (!this.cur) return;
      this.cur[field] = parseInt(hex.slice(1), 16);
      this.refreshTruth();
      this.renderAll();
    },

    colorSelectVal(field) {
      const v = this.cur?.[field];
      if (v === undefined || v === null) return "";
      return typeof v === "number" ? "custom" : v;
    },

    // ── Background helpers ────────────────────────────────────────────────────
    // These edit the top-level `spec.background` (NOT a widget field), so they are
    // always valid regardless of widget selection. `null`/absent = inherit theme.

    // Approximate theme-slot → preview hex map, mirroring widgets.js' THEME.
    _bgPreviewTheme: { primary: "#7aa2f7", secondary: "#bb9af7",
                       text: "#cdd6f4", background: "#1a1b26" },

    bgSelectVal() {
      const v = this.spec.background;
      if (v === undefined || v === null) return "inherit";
      return typeof v === "number" ? "custom" : v;
    },

    bgIsCustom() {
      return typeof this.spec.background === "number";
    },

    bgHex() {
      const v = this.spec.background;
      if (typeof v === "number") return "#" + v.toString(16).padStart(6, "0");
      return "#1a1b26";
    },

    // Resolve spec.background to a CSS colour for the editing canvas. Approximate;
    // the server truth preview is authoritative.
    bgCss() {
      const v = this.spec.background;
      if (typeof v === "number") return "#" + v.toString(16).padStart(6, "0");
      if (typeof v === "string") return this._bgPreviewTheme[v] || "#000";
      return "#000"; // inherit: device defaults to black until truth preview loads
    },

    setBgSlot(v) {
      if (v === "inherit") {
        this.spec.background = null;
      } else if (v === "custom") {
        this.spec.background = parseInt(this.bgHex().slice(1), 16);
      } else {
        this.spec.background = v;
      }
      this.refreshTruth();
      this.renderAll();
    },

    setBgHex(hex) {
      this.spec.background = parseInt(hex.slice(1), 16);
      this.refreshTruth();
      this.renderAll();
    },

    // ── Text source helpers ──────────────────────────────────────────────────

    setTextSrc(newSrc) {
      if (!this.cur) return;
      switch (newSrc) {
        case "hostname":
        case "uptime":
        case "ip":
        case "net_interface":
          this.cur.value = { src: newSrc };
          break;
        case "literal":
          this.cur.value = { src: "literal", fmt: "" };
          break;
        case "time":
          this.cur.value = { src: "time", fmt: "hhmm" };
          break;
        case "date":
          this.cur.value = { src: "date", fmt: "iso" };
          break;
        case "number":
          this.cur.value = { src: "number", fmt: { source: "cpu_percent", style: "percent" } };
          break;
      }
      this.refreshTruth();
      this.renderAll();
    },
  };
};
