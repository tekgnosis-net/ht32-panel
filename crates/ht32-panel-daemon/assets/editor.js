window.editor = function () {
  return {
    schema: { kinds: [] }, templates: [], name: "",
    spec: { name: "", orientation: "portrait", widgets: [] },
    sel: null, truthSrc: "", status: "", scale: 1.5,
    async init() {
      this.schema = await (await fetch("/api/template-schema")).json();
      this.templates = await (await fetch("/api/templates")).json();
      this.dims = this.spec.orientation.startsWith("portrait") ? [170,320] : [320,170];
    },
    deviceStyle() {
      const [w,h] = this.dims; const s = this.scale;
      return `width:${w*s}px;height:${h*s}px`;
    },
    widgetStyle(w) {
      const s = this.scale; const r = w.rect;
      return `left:${r.x*s}px;top:${r.y*s}px;width:${r.w*s}px;height:${r.h*s}px;`+
             `font-size:10px;color:#888;background:#222`;
    },
    select(i){ this.sel = i; },
    addWidget(){}, save(){}, load(){}, activate(){}, refreshTruth(){}, // Tasks 8/9/10/11
  };
};
