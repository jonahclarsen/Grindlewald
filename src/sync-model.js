// Per-record Lamport clocks and tombstones converge without trusting wall clocks.
export function newer(a, b) {
  return !b || a.clock > b.clock || (a.clock === b.clock && a.actor > b.actor);
}
export function mergeRecords(left, right) {
  const merged = structuredClone(left);
  for (const [id, record] of Object.entries(right)) if (newer(record, merged[id])) merged[id] = structuredClone(record);
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
          this.write(ids[oldKey], kind, null); delete ids[oldKey];
        }
      }
      this.state.baseline[kind] = structuredClone(settings[kind]);
    }
  }
  merge(records) {
    this.state.records = mergeRecords(this.state.records, records);
    this.state.clock = Math.max(this.state.clock, ...Object.values(records).map(record => record.clock));
  }
  materialize(settings) {
    const result = {...settings, devices:[], presets:[]};
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
