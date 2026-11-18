// Progressive enhancement: all screenshots and captions remain available without JS.
// This is a manual walkthrough, with no autoplay, tracking, or network requests.
for (const walkthrough of document.querySelectorAll('[data-walkthrough]')) {
  const tablist = walkthrough.querySelector('.tour-tabs');
  const tabs = [...tablist.querySelectorAll('button[data-panel]')];
  const panels = tabs.map(tab => document.getElementById(tab.dataset.panel));
  if (panels.some(panel => !panel)) continue;
  tablist.setAttribute('role', 'tablist');
  tabs.forEach((tab, index) => {
    tab.setAttribute('role', 'tab');
    tab.setAttribute('aria-controls', panels[index].id);
    panels[index].setAttribute('role', 'tabpanel');
    panels[index].setAttribute('aria-labelledby', tab.id);
    panels[index].tabIndex = 0;
  });
  function select(index, focus = false) {
    tabs.forEach((tab, current) => {
      const selected = current === index;
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
      panels[current].hidden = !selected;
    });
    if (focus) tabs[index].focus();
  }
  tabs.forEach((tab, index) => {
    tab.addEventListener('click', () => select(index));
    tab.addEventListener('keydown', event => {
      let next;
      if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
      if (event.key === 'ArrowLeft') next = (index + tabs.length - 1) % tabs.length;
      if (event.key === 'Home') next = 0;
      if (event.key === 'End') next = tabs.length - 1;
      if (next !== undefined) {
        event.preventDefault();
        select(next, true);
      }
    });
  });
  walkthrough.dataset.enhanced = 'true';
  function selectLinkedPanel() {
    const index = panels.findIndex(panel => '#' + panel.id === location.hash);
    if (index >= 0) select(index);
  }
  select(0);
  selectLinkedPanel();
  window.addEventListener('hashchange', selectLinkedPanel);
}

// Keep the original image URL available to browsers without modal dialogs and to
// anyone opening the link in a new tab. The high-resolution image loads on demand.
const screenshotLinks = document.querySelectorAll('a[data-screenshot]');
if (screenshotLinks.length && typeof window.HTMLDialogElement?.prototype.showModal === 'function') {
  const dialog = document.createElement('dialog');
  dialog.className = 'image-viewer';
  dialog.setAttribute('aria-labelledby', 'viewer-title');
  dialog.innerHTML = `
    <header class="viewer-toolbar">
      <h2 id="viewer-title"></h2>
      <button type="button" class="viewer-scale">Actual size</button>
      <button type="button" class="viewer-close" aria-label="Close image viewer" autofocus>
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18"/></svg>
      </button>
    </header>
    <div class="viewer-viewport" role="region" aria-label="Screenshot">
      <img alt="">
      <p class="viewer-error" role="status" hidden>Couldn’t load this screenshot. <a>Open the image file</a></p>
    </div>`;
  document.body.append(dialog);

  const title = dialog.querySelector('h2');
  const scale = dialog.querySelector('.viewer-scale');
  const close = dialog.querySelector('.viewer-close');
  const viewport = dialog.querySelector('.viewer-viewport');
  const photo = viewport.querySelector('img');
  const error = viewport.querySelector('.viewer-error');
  let opener;

  function setActualSize(actual, x = 0.5, y = 0.5) {
    dialog.classList.toggle('actual-size', actual);
    scale.textContent = actual ? 'Fit to window' : 'Actual size';
    viewport.tabIndex = actual ? 0 : -1;
    if (actual) {
      viewport.scrollLeft = photo.clientWidth * x - viewport.clientWidth / 2;
      viewport.scrollTop = photo.clientHeight * y - viewport.clientHeight / 2;
    } else {
      viewport.scrollTo(0, 0);
    }
  }

  for (const link of screenshotLinks) {
    link.setAttribute('aria-haspopup', 'dialog');
    link.addEventListener('click', event => {
      if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      opener = link;
      const thumbnail = link.querySelector('img');
      title.textContent = link.dataset.screenshot;
      photo.alt = thumbnail.alt;
      photo.width = Number(thumbnail.getAttribute('width'));
      photo.height = Number(thumbnail.getAttribute('height'));
      photo.hidden = false;
      photo.src = link.href;
      error.hidden = true;
      error.querySelector('a').href = link.href;
      scale.hidden = false;
      setActualSize(false);
      link.focus({ preventScroll: true });
      dialog.showModal();
    });
  }

  // Keep all controls reachable under macOS keyboard navigation preferences.
  dialog.addEventListener('keydown', event => {
    if (event.key !== 'Tab') return;
    const controls = [...dialog.querySelectorAll('button, a[href], [tabindex="0"]')]
      .filter(control => control.getClientRects().length);
    const index = controls.indexOf(document.activeElement);
    const next = event.shiftKey
      ? (index <= 0 ? controls.length - 1 : index - 1)
      : (index + 1) % controls.length;
    event.preventDefault();
    controls[next].focus({ preventScroll: true });
  });
  close.addEventListener('click', () => dialog.close());
  dialog.addEventListener('close', () => opener.focus({ preventScroll: true }));
  // Only dismiss a click that also began on the backdrop, so a pan ending
  // outside the frame never closes the viewer.
  let pressedBackdrop = false;
  dialog.addEventListener('pointerdown', event => { pressedBackdrop = event.target === dialog; });
  dialog.addEventListener('click', event => {
    if (pressedBackdrop && event.target === dialog) dialog.close();
    pressedBackdrop = false;
  });

  scale.addEventListener('click', () => {
    const actual = !dialog.classList.contains('actual-size');
    setActualSize(actual);
    if (actual) viewport.focus({ preventScroll: true });
  });
  photo.addEventListener('click', event => {
    const bounds = photo.getBoundingClientRect();
    setActualSize(!dialog.classList.contains('actual-size'),
      (event.clientX - bounds.left) / bounds.width, (event.clientY - bounds.top) / bounds.height);
  });
  photo.addEventListener('error', () => {
    setActualSize(false);
    photo.hidden = true;
    error.hidden = false;
    scale.hidden = true;
  });
}
