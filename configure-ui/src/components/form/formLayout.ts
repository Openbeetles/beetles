export type FormGridColumns = 1 | 2 | 3;
export type FormGridGap = "standard" | "compact";

const FORM_GRID_GAP: Record<FormGridGap, number> = {
  standard: 2,
  compact: 1.5,
};

export function createFormGridTemplateColumns(columns: FormGridColumns = 2) {
  return {
    xs: "minmax(0, 1fr)",
    md: columns === 1 ? "minmax(0, 1fr)" : `repeat(${columns}, minmax(0, 1fr))`,
  } as const;
}

export function createFormGridSx({
  columns = 2,
  gap = "standard",
}: {
  columns?: FormGridColumns;
  gap?: FormGridGap;
} = {}) {
  return {
    display: "grid",
    gap: FORM_GRID_GAP[gap],
    gridTemplateColumns: createFormGridTemplateColumns(columns),
    "& .MuiFormControl-root": { minWidth: 0 },
    "& .MuiTextField-root": { minWidth: 0 },
  } as const;
}
