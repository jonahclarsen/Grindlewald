// Per-record Lamport clocks and tombstones converge without trusting wall clocks.
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
  write(id, kind, value) {
    this.state.records[id] = {kind, value, clock:++this.state.clock, actor:this.state.actor};
  }
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
        const shared = kind === 'devices' ? {
          name:value.name, profile:value.profile, enabled:value.enabled,
          addresses:{...(this.state.records[id]?.value?.addresses ?? {}),
            ...(value.identifier.startsWith('UNPAIRED-') ? {} : {[this.platform]:value.identifier})},
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
        const record = id ? this.state.records[id] : undefined;
        return record ? (record.value ? [record.value.name] : []) : [name];
      });
      return {...schedule, ...(schedule.lights ? {lights} : {}), ...(preset ? {preset:preset.value?.name ?? ''} : {})};
    });
    this.state.ids = {devices:{},presets:{}};
    for (const [id, record] of Object.entries(this.state.records).sort(([a],[b]) => a.localeCompare(b))) {
      if (!record.value) continue;
      const value = record.kind === 'devices' ? {
        name:record.value.name, profile:record.value.profile, enabled:record.value.enabled,
        identifier:record.value.addresses[this.platform] ?? `UNPAIRED-${id}`,
      } : structuredClone(record.value);
      // Conflicting offline additions of the same name resolve deterministically.
      const existing = result[record.kind].findIndex(item => item.name === value.name);
      if (existing >= 0) result[record.kind].splice(existing, 1);
      result[record.kind].push(value);
      this.state.ids[record.kind][key(record.kind,value)] = id;
    }
    this.state.baseline = {devices:structuredClone(result.devices),presets:structuredClone(result.presets)};
    return result;
  }
  pair(settings, identifier, replacement) {
    const id = this.state.ids.devices[identifier];
    if (!id) throw Error('Sync this light first');
    const record = this.state.records[id];
    this.write(id, 'devices', {...record.value, addresses:{...record.value.addresses,[this.platform]:replacement}});
    return this.materialize(settings);
  }
}
