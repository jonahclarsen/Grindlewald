export const isMobile = /Android/i.test(globalThis.navigator?.userAgent ?? '');
if (isMobile) document.documentElement.dataset.platform = 'android';
export function prepareMobile() {
  if (!isMobile) return;
  const service = document.querySelector('.privileged-service-card');
  service?.previousElementSibling?.remove(); service?.remove();
  for (const selector of ['#automations-page', '[data-page="automations-page"]', '#service-notice', '#quit-button', '#remove-schedule-dialog', 'nav kbd', '.future-me-reminder', '.white-breathing-reminder']) {
    document.querySelectorAll(selector).forEach(node => node.remove());
  }
  document.querySelector('#mobile-settings').hidden = false;
  document.querySelector('#preset-editor').previousElementSibling.textContent = 'Your saved colors.';
  document.querySelector('#discovered-devices').previousElementSibling.textContent = 'Discover nearby lights or pair a synced light.';
}
export function startMobileUpdates(invoke) {
  const status = document.querySelector('#mobile-update-status');
  const check = async (manual) => {
    try { status.textContent = await invoke('mobile_action', { action: manual ? 'update' : 'auto-update' }); }
    catch { status.textContent = 'Could not check for updates'; }
  };
  document.querySelector('#mobile-update').onclick = () => check(true);
  document.querySelector('#mobile-permissions').onclick = () => invoke('mobile_action', {action:'permissions'});
  invoke('mobile_action', {action:'version'}).then(version => { document.querySelector('#mobile-version').textContent = version; });
  check(false);
  document.addEventListener('visibilitychange', () => { if (!document.hidden) check(false); });
}
