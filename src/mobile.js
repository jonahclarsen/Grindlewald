export const isMobile = /Android/i.test(globalThis.navigator?.userAgent ?? '');
if (isMobile) document.documentElement.dataset.platform = 'android';
export function prepareMobile() {
  if (!isMobile) return;
  const service = document.querySelector('.privileged-service-card');
  service?.previousElementSibling?.remove(); service?.remove();
  for (const selector of ['#automations-page', '[data-page="automations-page"]', '#shortcuts-page', '[data-page="shortcuts-page"]', '#service-notice', '#quit-button', '#remove-schedule-dialog', 'nav kbd']) {
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
let floodlightEmail = '';
/** True when this phone controls the floodlights through TP-Link itself instead of the paired Mac. */
export const hasFloodlightAccount = () => Boolean(floodlightEmail);
export function startFloodlightAccount(invoke, share = async () => {}) {
  const form = document.querySelector('#tplink-form');
  const status = document.querySelector('#tplink-status');
  const signOut = document.querySelector('#tplink-sign-out');
  const show = email => {
    floodlightEmail = email;
    status.textContent = email || 'For floodlights';
    form.hidden = Boolean(email); signOut.hidden = !email;
  };
  const account = async (request, busy) => {
    if (busy) status.textContent = busy;
    try { show(await invoke('floodlight_account', {request})); return true; }
    catch (error) { show(floodlightEmail); status.textContent = String(error); return false; }
  };
  form.onsubmit = async event => {
    event.preventDefault();
    const password = document.querySelector('#tplink-password');
    const login = {email: document.querySelector('#tplink-email').value.trim(), password: password.value};
    if (await account(login, 'Signing in…')) {
      password.value = '';
      share(login).catch(() => {}); // Paired devices pick this login up.
    }
  };
  signOut.onclick = () => account({signOut: true});
  const ready = account({});
  return {
    signedIn: hasFloodlightAccount,
    async signIn(login) { await ready; if (!floodlightEmail) await account(login, 'Signing in…'); },
  };
}
