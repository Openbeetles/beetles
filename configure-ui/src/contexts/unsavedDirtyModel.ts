export type DirtyOwner = string;
export type DirtyOwners = Set<DirtyOwner>;

function normalizeOwner(owner: DirtyOwner): DirtyOwner {
  return owner.trim();
}

export function setDirtyOwner(
  owners: DirtyOwners,
  owner: DirtyOwner,
  dirty: boolean,
): DirtyOwners {
  const normalized = normalizeOwner(owner);
  if (!normalized) return owners;
  const next = new Set(owners);
  if (dirty) next.add(normalized);
  else next.delete(normalized);
  return next;
}

export function clearDirtyOwner(
  owners: DirtyOwners,
  owner: DirtyOwner,
): DirtyOwners {
  return setDirtyOwner(owners, owner, false);
}

export function clearDirtyOwners(): DirtyOwners {
  return new Set();
}

export function isDirtyOwners(owners: DirtyOwners): boolean {
  return owners.size > 0;
}
