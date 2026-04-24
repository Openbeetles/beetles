import assert from "node:assert/strict";
import test from "node:test";
import {
  createFormGridSx,
  createFormGridTemplateColumns,
} from "./formLayout.ts";

test("createFormGridTemplateColumns collapses to one column on small screens", () => {
  assert.deepEqual(createFormGridTemplateColumns(2), {
    xs: "minmax(0, 1fr)",
    md: "repeat(2, minmax(0, 1fr))",
  });
});

test("createFormGridTemplateColumns keeps explicit one-column grids single column", () => {
  assert.deepEqual(createFormGridTemplateColumns(1), {
    xs: "minmax(0, 1fr)",
    md: "minmax(0, 1fr)",
  });
});

test("createFormGridSx centralizes field grid spacing and form-control min width", () => {
  assert.deepEqual(createFormGridSx({ columns: 3, gap: "compact" }), {
    display: "grid",
    gap: 1.5,
    gridTemplateColumns: {
      xs: "minmax(0, 1fr)",
      md: "repeat(3, minmax(0, 1fr))",
    },
    "& .MuiFormControl-root": { minWidth: 0 },
    "& .MuiTextField-root": { minWidth: 0 },
  });
});
