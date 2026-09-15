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
    panels[index].tabIndex = 0;
  });
  function select(index, focus = false) {
    tabs.forEach((tab, current) => {
      const selected = current === index;
      tab.setAttribute('aria-selected', String(selected));
      tab.tabIndex = selected ? 0 : -1;
      tab.classList.toggle('is-active', selected);
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
  select(1);
}
