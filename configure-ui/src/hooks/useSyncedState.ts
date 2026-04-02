import { useEffect, useState } from "react";

export function useSyncedState<T>(source: T) {
  const [state, setState] = useState(source);

  useEffect(() => {
    setState(source);
  }, [source]);

  return [state, setState] as const;
}
