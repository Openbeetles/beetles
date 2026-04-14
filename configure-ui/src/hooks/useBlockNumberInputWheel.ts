import { useEffect } from "react";

/**
 * 阻止滚轮改变 `input[type=number]` 的值（配置页数字项易误触）。
 * Blocks wheel from mutating numeric inputs; use keyboard or direct typing instead.
 */
export function useBlockNumberInputWheel() {
  useEffect(() => {
    const onWheel = (e: WheelEvent) => {
      const t = e.target;
      if (
        t instanceof HTMLInputElement &&
        t.type === "number" &&
        !t.disabled &&
        !t.readOnly
      ) {
        e.preventDefault();
      }
    };
    document.addEventListener("wheel", onWheel, { capture: true, passive: false });
    return () => {
      document.removeEventListener("wheel", onWheel, true);
    };
  }, []);
}
