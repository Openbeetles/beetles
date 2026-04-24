export interface LatestRequestGuard {
  next: () => number;
  isCurrent: (requestId: number) => boolean;
  invalidate: () => void;
}

export function createLatestRequestGuard(): LatestRequestGuard {
  let current = 0;
  return {
    next: () => {
      current += 1;
      return current;
    },
    isCurrent: (requestId: number) => requestId === current,
    invalidate: () => {
      current += 1;
    },
  };
}
