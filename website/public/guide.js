// The commands are selectable HTML even when the clipboard is unavailable.
if (window.isSecureContext && typeof navigator.clipboard?.writeText === 'function') {
  const commands = document.querySelectorAll('.guide-copy pre > code');
  commands.forEach((code, index) => {
    const toolbar = document.createElement('div');
    toolbar.className = 'command-tools';
    const status = document.createElement('span');
    status.setAttribute('role', 'status');
    const copy = document.createElement('button');
    copy.type = 'button';
    copy.textContent = 'Copy';
    copy.setAttribute('aria-label', `Copy command block ${index + 1}`);
    copy.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(code.textContent);
        status.textContent = 'Copied';
      } catch {
        status.textContent = 'Clipboard unavailable. Select the command and copy it manually.';
      }
    });
    toolbar.append(status, copy);
    code.parentElement.before(toolbar);
  });
}
