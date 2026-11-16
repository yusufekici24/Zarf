import { splitNameAddr } from "$lib/format";

export type SchedulingPerson = { name: string; addr: string; role: string; group?: string; unresolved?: boolean };
type Directory = { groupAddresses: (emails: string[]) => Promise<string[]>; expandGroup: (email: string) => Promise<string[]> };

/** Expand only for display; the invitation retains its original distribution lists. */
export async function resolveSchedulingPeople(input: SchedulingPerson[], directory: Directory) {
  const people = new Map<string, SchedulingPerson>();
  const warnings = new Set<string>();
  const expanded = new Map<string, string[]>();
  let remaining = 2000;
  const add = (p: SchedulingPerson) => {
    const old = people.get(p.addr);
    if (!old || (old.role === "opt" && p.role === "req")) people.set(p.addr, p);
  };
  async function visit(batch: SchedulingPerson[], path: Set<string>) {
    if (!batch.length) return;
    const normalized = batch.map(p => ({ ...p, addr: p.addr.trim().toLowerCase() }));
    const groups = new Set<string>();
    for (let i = 0; i < normalized.length; i += 200) {
      for (const addr of await directory.groupAddresses(normalized.slice(i, i + 200).map(p => p.addr))) groups.add(addr.toLowerCase());
    }
    for (const p of normalized) {
      if (!groups.has(p.addr) || p.role === "room") { add(p); continue; }
      try {
        if (path.has(p.addr) || path.size >= 10 || remaining <= 0) throw new Error("limit");
        let members = expanded.get(p.addr);
        if (!members) { members = await directory.expandGroup(p.addr); expanded.set(p.addr, members); }
        if (!members.length || members.length > remaining) throw new Error("empty/limit");
        remaining -= members.length;
        await visit(members.map(a => ({ ...splitNameAddr(a), role: p.role, group: p.group || p.name || p.addr })), new Set([...path, p.addr]));
      } catch {
        warnings.add(p.name || p.addr);
        add({ ...p, unresolved: true });
      }
    }
  }
  await visit(input, new Set());
  return { people: [...people.values()], warnings: [...warnings] };
}

export function combinedAvailability(states: number[]): number {
  if (!states.length || states.includes(4)) return 4;
  return Math.max(...states);
}
