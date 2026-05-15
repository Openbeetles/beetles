import type { ReactNode } from "react";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import {
  CONFIG_PANEL_TAB_INDICATOR_SX,
  createConfigPanelTabsSx,
} from "./ConfigPanelTabs.styles";

export type ConfigPanelTabValue = string | number;

export type ConfigPanelTabItem<T extends ConfigPanelTabValue> = {
  value: T;
  label: ReactNode;
  disabled?: boolean;
};

type ConfigPanelTabsProps<T extends ConfigPanelTabValue> = {
  value: T;
  items: readonly ConfigPanelTabItem<T>[];
  ariaLabel: string;
  onChange: (value: T) => void;
  minTabWidth?: number;
};

export function ConfigPanelTabs<T extends ConfigPanelTabValue>({
  value,
  items,
  ariaLabel,
  onChange,
  minTabWidth,
}: ConfigPanelTabsProps<T>) {
  return (
    <Tabs
      value={value}
      onChange={(_, nextValue) => onChange(nextValue)}
      variant="scrollable"
      scrollButtons={false}
      aria-label={ariaLabel}
      TabIndicatorProps={{ sx: CONFIG_PANEL_TAB_INDICATOR_SX }}
      sx={createConfigPanelTabsSx({ minTabWidth })}
    >
      {items.map((item) => (
        <Tab
          key={item.value}
          value={item.value}
          label={item.label}
          disabled={item.disabled}
          disableRipple
        />
      ))}
    </Tabs>
  );
}
