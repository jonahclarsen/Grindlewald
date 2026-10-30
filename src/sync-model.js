// Per-record Lamport clocks and tombstones converge without trusting wall clocks.
// Govee lights advertise the last two bytes of their Bluetooth address
// (ihoment_H6005_1A2B). macOS hides that address behind a per-host UUID, so this
// suffix is the only identity both platforms can see.
const ADVERTISED = /_H[0-9A-Z]{4}_([0-9A-F]{4})$/i;
export function hardwareHint(name, address) {
  const mac = /^(?:[0-9A-F]{2}:){4}([0-9A-F]{2}):([0-9A-F]{2})$/i.exec(address ?? '');
  return (mac ? mac[1] + mac[2] : ADVERTISED.exec(name ?? '')?.[1])?.toUpperCase();
}
const hintOf = value => value.hint ?? hardwareHint(value.name, value.addresses.android);
export function newer(a, b) {
  return !b || a.clock > b.clock || (a.clock === b.clock && a.actor > b.actor);
}
export function mergeRecords(left, right) {
  const merged = structuredClone(left);
  for (const [id, record] of Object.entries(right)) {
    const current = merged[id];
    const winner = newer(record, current) ? record : current;
    merged[id] = structuredClone(winner);
    // Platforms own independent addresses. An offline rename must not erase a
    // newly paired address on the other platform.
    if (current?.kind === 'devices' && record.kind === 'devices' && current.value && record.value) {
      const loser = winner === record ? current : record;
      const addresses = {...loser.value.addresses, ...winner.value.addresses};
      merged[id].value.addresses = Object.fromEntries(Object.entries(addresses).sort(([a],[b]) => a.localeCompare(b)));
      if (!merged[id].value.hint && loser.value.hint) merged[id].value.hint = loser.value.hint;
    }
  }
  return merged;
}
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const key = (kind, value) => kind === 'devices' ? value.identifier : value.name;
export class SyncModel {
  constructor(platform, stored = {}, randomId = () => crypto.randomUUID()) {
    this.platform = platform; this.randomId = randomId;
    this.state = {actor:randomId(), clock:0, records:{}, baseline:{devices:[],presets:[]}, ids:{devices:{},presets:{}}, ...structuredClone(stored)};
  }
  write(id, kind, value, extra = {}) {
    this.state.records[id] = {kind, value, clock:++this.state.clock, actor:this.state.actor, ...extra};
  }
  // Follows lights combined into another record so local references survive.
  resolveId(id) {
    for (let hops = 0; this.state.records[id] && !this.state.records[id].value && this.state.records[id].mergedInto && hops < 8; hops++) id = this.state.records[id].mergedInto;
    return id;
  }
  resolve(id) { return this.state.records[this.resolveId(id)]; }
  capture(settings) {
    for (const kind of ['devices', 'presets']) {
      const previous = this.state.baseline[kind];
      const ids = this.state.ids[kind];
      for (const value of settings[kind]) {
        const localKey = key(kind, value);
        let id = ids[localKey];
        // A renamed light retains its local identifier. Preset renames are delete + add.
        if (!id) ids[localKey] = id = this.randomId();
        const old = previous.find(item => key(kind,item) === localKey);
        if (same(old, value)) continue;
        const existing = this.state.records[id]?.value;
        const paired = kind === 'devices' && !value.identifier.startsWith('UNPAIRED-');
        const hint = existing?.hint ?? (paired ? hardwareHint(value.name, value.identifier) : undefined);
        const shared = kind === 'devices' ? {
          name:value.name, profile:value.profile, enabled:value.enabled,
          addresses:{...(existing?.addresses ?? {}), ...(paired ? {[this.platform]:value.identifier} : {})},
          ...(hint ? {hint} : {}),
        } : {name:value.name,mode:value.mode,value:value.value,brightness:value.brightness};
        this.write(id, kind, shared);
      }
      for (const old of previous) {
        const oldKey = key(kind,old);
        if (!settings[kind].some(value => key(kind,value) === oldKey)) {
          for (const [id, record] of Object.entries(this.state.records)) {
            if (id === ids[oldKey] || (record.kind === kind && record.value?.name === old.name)) this.write(id, kind, null);
          }
          delete ids[oldKey];
        }
      }
      this.state.baseline[kind] = structuredClone(settings[kind]);
    }
  }
  merge(records) {
    this.state.records = mergeRecords(this.state.records, records);
    this.state.clock = Math.max(this.state.clock, ...Object.values(records).map(record => record.clock));
  }
  renamePreset(oldName, name) {
    const id = this.state.ids.presets[oldName];
    const record = this.state.records[id];
    if (!record?.value || oldName === name) return;
    this.write(id, 'presets', {...record.value, name});
    delete this.state.ids.presets[oldName];
    this.state.ids.presets[name] = id;
    const previous = this.state.baseline.presets.find(preset => preset.name === oldName);
    if (previous) previous.name = name;
  }
  materialize(settings) {
    const previousIds = this.state.ids;
    const result = {...settings, devices:[], presets:[]};
    // Rewrite local automation references just as a rename in Settings does;
    // automation contents never enter the shared records.
    if (settings.schedules) result.schedules = settings.schedules.map(schedule => {
      const presetId = previousIds.presets[schedule.preset];
      const preset = presetId ? this.state.records[presetId] : undefined;
      const lights = (schedule.lights ?? []).flatMap(name => {
        const device = settings.devices.find(device => device.name === name);
        const id = device && previousIds.devices[device.identifier];
        const record = id ? this.resolve(id) : undefined;
        return record ? (record.value ? [record.value.name] : []) : [name];
      });
      return {...schedule, ...(schedule.lights ? {lights} : {}), ...(preset ? {preset:preset.value?.name ?? ''} : {})};
    });
    this.state.ids = {devices:{},presets:{}};
    for (const [id, record] of Object.entries(this.state.records).sort(([a],[b]) => a.localeCompare(b))) {
      if (!record.value) continue;
      const value = record.kind === 'devices' ? {
        name:record.value.name, profile:record.value.profile, enabled:record.value.enabled,
        identifier:record.value.addresses[this.platform] ?? `UNPAIRED-${id.toUpperCase()}`,
      } : structuredClone(record.value);
      // Conflicting offline additions of the same name resolve deterministically.
      const existing = result[record.kind].findIndex(item => item.name === value.name);
      if (existing >= 0) result[record.kind].splice(existing, 1);
      result[record.kind].push(value);
      this.state.ids[record.kind][key(record.kind,value)] = id;
    }
    // Light order is per platform: each keeps its own arrangement and appends new lights.
    const rank = new Map(settings.devices.map((device, index) => [this.resolveId(previousIds.devices[device.identifier]), index]));
    const rankOf = device => rank.get(this.state.ids.devices[device.identifier]) ?? Infinity;
    result.devices.sort((a, b) => rankOf(a) - rankOf(b));
    this.state.baseline = {devices:structuredClone(result.devices),presets:structuredClone(result.presets)};
    return result;
  }
  // Records learn their hardware hint from discovery. A discovered light that
  // matches a light synced from the other platform pairs with it automatically.
  observe(discovered) {
    let changed = false;
    for (const device of discovered) {
      const hint = hardwareHint(device.name, device.identifier);
      if (!hint) continue;
      const id = this.state.ids.devices[device.identifier];
      const record = id && this.state.records[id];
      if (record?.value) {
        if (record.value.hint !== hint) { this.write(id, 'devices', {...record.value, hint}); changed = true; }
        continue;
      }
      const match = Object.entries(this.state.records).sort(([a],[b]) => a.localeCompare(b)).find(([, record]) =>
        record.kind === 'devices' && record.value && !record.value.addresses[this.platform] && hintOf(record.value) === hint);
      if (match) {
        this.write(match[0], 'devices', {...match[1].value, hint, addresses:{...match[1].value.addresses, [this.platform]:device.identifier}});
        changed = true;
      }
    }
    return this.combineDuplicates() || changed;
  }
  // The same light added separately on each platform becomes one record that
  // carries both addresses. Every device reaches the same result independently.
  combineDuplicates() {
    const groups = new Map();
    for (const [id, record] of Object.entries(this.state.records).sort(([a],[b]) => a.localeCompare(b))) {
      const hint = record.kind === 'devices' && record.value && hintOf(record.value);
      if (hint) groups.set(hint, [...(groups.get(hint) ?? []), id]);
    }
    let changed = false;
    for (const [hint, [survivorId, ...others]] of groups) {
      for (const id of others) {
        const survivor = this.state.records[survivorId].value, other = this.state.records[id].value;
        // Different addresses on one platform are two lights sharing a suffix.
        if (Object.entries(other.addresses).some(([platform, address]) => survivor.addresses[platform] && survivor.addresses[platform] !== address)) continue;
        const name = [survivor.name, other.name].find(name => !ADVERTISED.test(name)) ?? survivor.name;
        const addresses = Object.fromEntries(Object.entries({...other.addresses, ...survivor.addresses}).sort(([a],[b]) => a.localeCompare(b)));
        this.write(survivorId, 'devices', {...survivor, name, enabled:survivor.enabled || other.enabled, addresses, hint});
        this.write(id, 'devices', null, {mergedInto:survivorId});
        changed = true;
      }
    }
    return changed;
  }
  pair(settings, identifier, replacement) {
    const id = this.state.ids.devices[identifier];
    if (!id) throw Error('Sync this light first');
    const record = this.state.records[id];
    this.write(id, 'devices', {...record.value, addresses:{...record.value.addresses,[this.platform]:replacement}});
    return this.materialize(settings);
  }
}
