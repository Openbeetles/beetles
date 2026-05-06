import { useEffect, useState } from "react";

export function useSyncedState<T>(source: T) {
  const [state, setState] = useState(source);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- this hook intentionally mirrors newly loaded device config into an editable draft.
    setState(source);
  }, [source]);

  return [state, setState] as const;
}
