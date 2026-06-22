// Approximate, smooth client-side renderer for the editing canvas.
// NOT authoritative — the server PNG is. Values are illustrative sample data.
// Exposed as a global (no ES modules) so classic-script editor.js can call it.

(function () {
  var SAMPLE = { cpu_percent: 65, ram_percent: 80, cpu_temp: 72,
                 disk_history: 45, net_in: 30, net_out: 20 };

  var THEME = { primary: "#7aa2f7", secondary: "#bb9af7",
                text: "#cdd6f4", background: "#1a1b26", accent: "#f7768e" };

  function resolveColor(c) {
    if (typeof c === "string") return THEME[c] || c;
    // integer: encode as 0xRRGGBB
    if (typeof c === "number") {
      var hex = c.toString(16).padStart(6, "0");
      return "#" + hex;
    }
    return "#7aa2f7";
  }

  function previewText(value) {
    if (typeof value === "object" && value !== null) {
      if (value.src === "literal") return value.fmt || "text";
      return "{" + value.src + "}";
    }
    return String(value);
  }

  function renderText(el, w) {
    el.style.cssText += ";display:flex;align-items:center;overflow:hidden;";
    el.style.color = resolveColor(w.color || "text");
    var sz = w.size ? Math.min(w.size, el.clientHeight || 14) : 11;
    el.style.fontSize = sz + "px";
    el.style.justifyContent = w.align === "right" ? "flex-end"
                            : w.align === "center" ? "center" : "flex-start";
    el.textContent = previewText(w.value);
  }

  function renderClock(el, w) {
    var c = document.createElement("canvas");
    c.width  = el.clientWidth  || 80;
    c.height = el.clientHeight || 20;
    el.appendChild(c);
    var g = c.getContext("2d");
    var color = resolveColor(w.color || "text");
    g.fillStyle = color;
    var sz = Math.floor(c.height * 0.6);
    g.font = sz + "px monospace";
    g.textBaseline = "middle";
    g.fillText("14:30", 2, c.height / 2);
  }

  function renderBar(el, w) {
    var pct = SAMPLE[w.value] !== undefined ? SAMPLE[w.value] : 50;
    pct = Math.max(0, Math.min(100, pct));
    var bgColor   = resolveColor(w.bg   !== undefined ? w.bg   : "background");
    var fillColor = resolveColor(w.fill !== undefined ? w.fill : "primary");
    el.style.background = bgColor;
    el.style.overflow = "hidden";
    var fill = document.createElement("div");
    fill.style.cssText = "height:100%;width:" + pct + "%;background:" + fillColor + ";";
    el.appendChild(fill);
  }

  function renderGauge(el, w) {
    var c = document.createElement("canvas");
    c.width  = el.clientWidth  || 40;
    c.height = el.clientHeight || 20;
    el.appendChild(c);
    var g = c.getContext("2d");
    var pct = SAMPLE[w.value] !== undefined ? SAMPLE[w.value] : 50;
    var lo  = typeof w.min === "number" ? w.min : 0;
    var hi  = typeof w.max === "number" ? w.max : 100;
    var frac = hi > lo ? (pct - lo) / (hi - lo) : 0.5;
    frac = Math.max(0, Math.min(1, frac));

    var cx = c.width / 2, cy = c.height * 0.75;
    var r  = Math.min(c.width, c.height) * 0.55;
    var startAngle = Math.PI * 0.8;
    var endAngle   = Math.PI * 2.2;
    var fillEnd    = startAngle + frac * (endAngle - startAngle);

    // track
    g.beginPath();
    g.arc(cx, cy, r, startAngle, endAngle);
    g.strokeStyle = resolveColor(w.track || "background");
    g.lineWidth = Math.max(2, r * 0.25);
    g.stroke();

    // fill
    if (frac > 0) {
      g.beginPath();
      g.arc(cx, cy, r, startAngle, fillEnd);
      g.strokeStyle = resolveColor(w.color || "primary");
      g.lineWidth = Math.max(2, r * 0.25);
      g.stroke();
    }
  }

  function renderSparkline(el, w) {
    var c = document.createElement("canvas");
    c.width  = el.clientWidth  || 80;
    c.height = el.clientHeight || 20;
    el.appendChild(c);
    var g = c.getContext("2d");

    // generate a pseudo-random wavy line using the source name as seed
    var seed  = 0;
    var src   = (w.a || "data");
    for (var s = 0; s < src.length; s++) { seed += src.charCodeAt(s); }

    var pts = [];
    var n   = Math.max(c.width, 8);
    for (var x = 0; x < n; x++) {
      // deterministic "noise" from seed + x
      var v = (Math.sin(x * 0.4 + seed) * 0.4 +
               Math.sin(x * 0.13 + seed * 2) * 0.3 +
               0.5);
      v = Math.max(0, Math.min(1, v));
      pts.push([x, c.height - v * (c.height - 2) - 1]);
    }

    var colorA = resolveColor(w.color_a || "primary");
    g.beginPath();
    g.moveTo(pts[0][0], pts[0][1]);
    for (var p = 1; p < pts.length; p++) { g.lineTo(pts[p][0], pts[p][1]); }
    g.strokeStyle = colorA;
    g.lineWidth = 1.5;
    g.stroke();
  }

  window.renderWidget = function renderWidget(el, w) {
    el.innerHTML = "";
    if (!w || !w.kind) return;
    switch (w.kind) {
      case "text":      renderText(el, w);      break;
      case "clock":     renderClock(el, w);     break;
      case "bar":       renderBar(el, w);       break;
      case "gauge":     renderGauge(el, w);     break;
      case "sparkline": renderSparkline(el, w); break;
    }
  };
}());
