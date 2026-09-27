// 品牌卡模板共享运行时：按 window.CARD 填文案、品牌色、logo 与字体，再按容器缩字防溢出。
// 只用 textContent 写文案，不拼 HTML；资源只来自 brand 虚拟主机，模板 CSP 禁止其他网络访问。
(function () {
  const card = window.CARD || {};
  const root = document.documentElement;
  root.dataset.aspect = card.aspect || "portrait";
  root.style.setProperty("--primary", card.primaryColor || "#FFFFFF");
  root.style.setProperty("--accent", card.accentColor || "#D9E2EC");

  const slots = card.slots || {};
  document.querySelectorAll("[data-slot]").forEach((element) => {
    const value = slots[element.dataset.slot];
    if (value) element.textContent = value;
    else element.remove();
  });
  document.querySelectorAll("[data-brand]").forEach((element) => {
    const value = card[element.dataset.brand];
    if (value) element.textContent = value;
    else element.remove();
  });
  document.querySelectorAll("[data-when-empty]").forEach((element) => {
    if (!element.querySelector("[data-slot],[data-brand],[data-logo]")) element.remove();
  });

  const logo = document.querySelector("[data-logo]");
  let logoReady = Promise.resolve(null);
  if (logo && card.logoUrl) {
    logo.src = card.logoUrl;
    logoReady = logo.decode().then(() => null, () => "logo");
  } else if (logo) {
    logo.remove();
  }

  let fontReady = Promise.resolve(null);
  if (card.fontUrl) {
    fontReady = new FontFace("BrandFont", `url("${card.fontUrl}")`)
      .load()
      .then((face) => {
        document.fonts.add(face);
        root.classList.add("brand-font");
        return null;
      }, () => "font");
  }

  // 只和 max-height 比：紧行高下字形本身会溢出行框，那不算放不下，留 0.3em 余量。
  function fit() {
    document.querySelectorAll("[data-fit]").forEach((element) => {
      const style = getComputedStyle(element);
      let size = parseFloat(style.fontSize);
      const floor = size * 0.5;
      const maxHeight = parseFloat(style.maxHeight);
      const overflows = () =>
        element.scrollWidth > element.clientWidth + 1 ||
        (Number.isFinite(maxHeight) && element.scrollHeight > maxHeight + size * 0.3);
      while (size > floor && overflows()) {
        size *= 0.94;
        element.style.fontSize = `${size}px`;
      }
    });
  }

  window.__cardReady = Promise.all([logoReady, fontReady])
    .then((failures) => document.fonts.ready.then(() => failures.filter(Boolean)))
    .then((failures) => {
      fit();
      return { ok: failures.length === 0, failures };
    });
})();
