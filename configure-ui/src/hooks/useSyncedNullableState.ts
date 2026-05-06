import { useEffect, useState } from "react";

export function useSyncedNullableState<T>(source: T | null) {
  const [state, setState] = useState<T | null>(source);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- this hook intentionally mirrors newly loaded device config into an editable draft.
    setState(source);
  }, [source]);

  return [state, setState] as const;
}
