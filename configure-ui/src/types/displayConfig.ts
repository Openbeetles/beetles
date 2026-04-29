export type DisplayDriver = 'st7789' | 'ili9341' | 'st7735' | 'framebuffer'
export type DisplayBus = 'spi' | 'framebuffer'
export type DisplayColorOrder = 'rgb' | 'bgr'

export interface DisplaySpiConfig {
  host: 2 | 3
  sclk: number
  mosi: number
  cs: number
  dc: number
  rst: number | null
  bl: number | null
  freq_hz: number
}

export interface DisplayConfig {
  version: number
  enabled: boolean
  driver: DisplayDriver
  bus: DisplayBus
  width: number
  height: number
  rotation: 0 | 90 | 180 | 270
  color_order: DisplayColorOrder
  invert_colors: boolean
  linux_spi_swap_bytes: boolean
  offset_x: number
  offset_y: number
  spi: DisplaySpiConfig
  /** Linux 设备路径：framebuffer 模式为 /dev/fb0，SPI 模式可填 /dev/spidev0.0 */
  fb_device: string
  /** sysfs 背光亮度文件路径，如 /sys/class/backlight/xxx/brightness；空则关闭背光控制 */
  backlight_sysfs: string | null
  sleep_timeout_secs: number
}

export function defaultDisplayConfig(): DisplayConfig {
  return {
    version: 1,
    enabled: false,
    driver: 'st7789',
    bus: 'spi',
    width: 240,
    height: 240,
    rotation: 0,
    color_order: 'rgb',
    invert_colors: false,
    linux_spi_swap_bytes: false,
    offset_x: 0,
    offset_y: 0,
    spi: {
      host: 2,
      sclk: 42,
      mosi: 41,
      cs: 21,
      dc: 40,
      rst: null,
      bl: null,
      freq_hz: 40_000_000,
    },
    fb_device: '/dev/fb0',
    backlight_sysfs: null,
    sleep_timeout_secs: 0,
  }
}

/** 合并 API 返回（可能缺省 serde 新字段）为完整 DisplayConfig。 */
export function normalizeDisplayConfig(
  input: Partial<DisplayConfig> & Record<string, unknown>,
): DisplayConfig {
  const d = defaultDisplayConfig()
  const spiIn = (input.spi ?? {}) as Partial<DisplaySpiConfig>
  const blRaw = input.backlight_sysfs
  let backlight_sysfs: string | null = d.backlight_sysfs
  if (blRaw === null || blRaw === undefined) {
    backlight_sysfs = null
  } else if (typeof blRaw === 'string') {
    const t = blRaw.trim()
    backlight_sysfs = t === '' ? null : t
  }

  let driver = input.driver ?? d.driver
  if (driver !== 'st7789' && driver !== 'ili9341' && driver !== 'st7735' && driver !== 'framebuffer') {
    driver = d.driver
  }
  let bus = input.bus ?? d.bus
  if (bus !== 'spi' && bus !== 'framebuffer') {
    bus = d.bus
  }

  return {
    ...d,
    ...input,
    version: typeof input.version === 'number' ? input.version : d.version,
    enabled: Boolean(input.enabled),
    driver,
    bus,
    width: typeof input.width === 'number' ? input.width : d.width,
    height: typeof input.height === 'number' ? input.height : d.height,
    rotation: [0, 90, 180, 270].includes(input.rotation as number)
      ? (input.rotation as DisplayConfig['rotation'])
      : d.rotation,
    color_order:
      input.color_order === 'bgr' || input.color_order === 'rgb'
        ? input.color_order
        : d.color_order,
    invert_colors: Boolean(input.invert_colors),
    linux_spi_swap_bytes: Boolean(input.linux_spi_swap_bytes),
    offset_x: typeof input.offset_x === 'number' ? input.offset_x : d.offset_x,
    offset_y: typeof input.offset_y === 'number' ? input.offset_y : d.offset_y,
    spi: {
      ...d.spi,
      ...spiIn,
      host: spiIn.host === 3 ? 3 : 2,
      rst: spiIn.rst === undefined ? d.spi.rst : spiIn.rst,
      bl: spiIn.bl === undefined ? d.spi.bl : spiIn.bl,
    },
    fb_device:
      typeof input.fb_device === 'string' && input.fb_device.trim() !== ''
        ? input.fb_device.trim()
        : d.fb_device,
    backlight_sysfs,
    sleep_timeout_secs:
      typeof input.sleep_timeout_secs === 'number'
        ? input.sleep_timeout_secs
        : d.sleep_timeout_secs,
  }
}
