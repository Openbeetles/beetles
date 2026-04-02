import { useEffect, useState } from "react";

export function useSyncedNullableState<T>(source: T | null) {
  const [state, setState] = useState<T | null>(source);

  useEffect(() => {
    setState(source);
  }, [source]);

  return [state, setState] as const;
}
