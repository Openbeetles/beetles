/**
 * 与固件 `config::HardwareSegment` / `DeviceEntry` 对齐，对应 GET/POST /api/config/hardware。
 */

export type PinConfig = Record<string, number>

export interface DeviceEntry {
  id: string
  device_type: string
  pins: PinConfig
  what: string
  how: string
  /** pwm_out 可含 frequency_hz 等，见固件校验 */
  options?: Record<string, unknown>
}

export interface I2cBusConfig {
  sda_pin: number
  scl_pin: number
  freq_hz?: number
}

export interface I2cDeviceEntry {
  id: string
  addr: number
  what: string
  how: string
  options?: Record<string, unknown>
}

/** 与固件 `I2cSensorEntry` 对齐 */
export interface I2cSensorEntry {
  id: string
  addr: number
  model: string
  watch_field: string
  what: string
  how: string
  options?: Record<string, unknown>
}

export interface HardwareSegment {
  hardware_devices: DeviceEntry[]
  i2c_bus?: I2cBusConfig | null
  i2c_devices?: I2cDeviceEntry[]
  i2c_sensors?: I2cSensorEntry[]
}

export const HARDWARE_DEVICE_TYPES = [
  'gpio_out',
  'gpio_in',
  'pwm_out',
  'adc_in',
  'buzzer',
  'dht',
] as const

export type HardwareDeviceType = (typeof HARDWARE_DEVICE_TYPES)[number]

export const MAX_HARDWARE_DEVICES = 8
export const MAX_PWM_DEVICES = 4
export const HARDWARE_PIN_MIN = 1
export const HARDWARE_PIN_MAX = 48
export const HARDWARE_FORBIDDEN_PINS = [0, 3, 46] as const
export const HARDWARE_ADC1_MAX_PIN = 10
export const HARDWARE_PWM_FREQ_MIN = 1
export const HARDWARE_PWM_FREQ_MAX = 40_000

export const MAX_I2C_SENSORS = 8
export const I2C_SENSOR_ID_MAX_LEN = 64
export const I2C_SENSOR_ADDR_MIN = 0x08
export const I2C_SENSOR_ADDR_MAX = 0x77
export const I2C_SENSOR_MAX_CMD_LEN = 4
export const I2C_MAX_READ_LEN_UI = 32
export const I2C_BUS_FREQ_MIN = 10_000
export const I2C_BUS_FREQ_MAX = 1_000_000
export const I2C_BUS_DEFAULT_FREQ_HZ = 100_000
export const I2C_BUS_DEFAULT_SDA_PIN = 21
export const I2C_BUS_DEFAULT_SCL_PIN = 22

export const I2C_SENSOR_MODELS = ['sht3x', 'aht20', 'raw'] as const
export type I2cSensorModel = (typeof I2C_SENSOR_MODELS)[number]

export function defaultHardwareSegment(): HardwareSegment {
  return {
    hardware_devices: [],
    i2c_sensors: [],
  }
}
