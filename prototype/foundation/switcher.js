/*
 * 原型预览切换器：配色方案 + 明暗模式。选择存 localStorage，跨页面保持。
 * 仅用于原型预览，不代表真实运行时能力。
 */
(function () {
  var PALETTES = [
    { id: "linear", name: "蓝紫 Linear" },
    { id: "neutral", name: "中性无彩" },
  ];
  var root = document.documentElement;
  var savedPalette = localStorage.getItem("proto-palette") || "linear";
  var savedTheme = localStorage.getItem("proto-theme") || "light";
  var savedOs = localStorage.getItem("proto-os") || "macos";
  root.dataset.palette = savedPalette;
  root.dataset.theme = savedTheme;
  root.dataset.os = savedOs;

  function applyPalette(id) {
    root.dataset.palette = id;
    localStorage.setItem("proto-palette", id);
    render();
  }
  function applyTheme(t) {
    root.dataset.theme = t;
    localStorage.setItem("proto-theme", t);
    render();
  }
  function applyOs(o) {
    root.dataset.os = o;
    localStorage.setItem("proto-os", o);
    render();
  }

  var bar;
  function render() {
    if (!bar) return;
    bar.innerHTML = "";
    var label = document.createElement("span");
    label.textContent = "配色";
    label.className = "sw-label";
    bar.appendChild(label);
    PALETTES.forEach(function (p) {
      var b = document.createElement("button");
      b.textContent = p.name;
      b.className = "sw-btn" + (root.dataset.palette === p.id ? " active" : "");
      b.onclick = function () { applyPalette(p.id); };
      bar.appendChild(b);
    });
    var sep = document.createElement("span");
    sep.className = "sw-sep";
    bar.appendChild(sep);
    [["light", "浅色"], ["dark", "深色"]].forEach(function (t) {
      var b = document.createElement("button");
      b.textContent = t[1];
      b.className = "sw-btn" + (root.dataset.theme === t[0] ? " active" : "");
      b.onclick = function () { applyTheme(t[0]); };
      bar.appendChild(b);
    });
    var sep2 = document.createElement("span");
    sep2.className = "sw-sep";
    bar.appendChild(sep2);
    [["macos", "macOS"], ["windows", "Windows"]].forEach(function (o) {
      var b = document.createElement("button");
      b.textContent = o[1];
      b.className = "sw-btn" + (root.dataset.os === o[0] ? " active" : "");
      b.onclick = function () { applyOs(o[0]); };
      bar.appendChild(b);
    });
    var home = document.createElement("a");
    home.textContent = "← 索引";
    home.className = "sw-home";
    home.href = "../../index.html";
    bar.appendChild(home);
  }

  window.addEventListener("DOMContentLoaded", function () {
    bar = document.createElement("div");
    bar.id = "proto-switcher";
    document.body.appendChild(bar);
    render();
  });
})();
