import type { HardwareSegment } from "../types/hardwareConfig.ts";
import {
  HARDWARE_ADC1_MAX_PIN,
  HARDWARE_DEVICE_TYPES,
  HARDWARE_FORBIDDEN_PINS,
  HARDWARE_PIN_MAX,
  HARDWARE_PIN_MIN,
  HARDWARE_PWM_FREQ_MAX,
  HARDWARE_PWM_FREQ_MIN,
  I2C_BUS_FREQ_MAX,
  I2C_BUS_FREQ_MIN,
  I2C_MAX_READ_LEN_UI,
  I2C_SENSOR_ADDR_MAX,
  I2C_SENSOR_ADDR_MIN,
  I2C_SENSOR_ID_MAX_LEN,
  I2C_SENSOR_MAX_CMD_LEN,
  I2C_SENSOR_MODELS,
  MAX_HARDWARE_DEVICES,
  MAX_I2C_SENSORS,
  MAX_PWM_DEVICES,
  type I2cSensorModel,
} from "../types/hardwareConfig.ts";

export function validateHardwareSegment(
  segment: HardwareSegment,
  t: (key: string) => string,
): string | null {
  const devices = segment.hardware_devices;
  if (devices.length > MAX_HARDWARE_DEVICES) {
    return t("hardwareConfig.validation.maxDevices");
  }

  const seenDeviceIds = new Set<string>();
  const seenPins = new Set<number>();
  let pwmCount = 0;
  for (const device of devices) {
    if (!device.id.trim() || device.id.length > 32) {
      return t("hardwareConfig.validation.idLen");
    }
    if (seenDeviceIds.has(device.id)) {
      return t("hardwareConfig.validation.idDup");
    }
    seenDeviceIds.add(device.id);
    if (!HARDWARE_DEVICE_TYPES.includes(device.device_type as never)) {
      return t("hardwareConfig.validation.badType");
    }

    const pin = device.pins.pin;
    if (pin == null || !Number.isFinite(pin)) {
      return t("hardwareConfig.validation.pinRequired");
    }
    if (pin < HARDWARE_PIN_MIN || pin > HARDWARE_PIN_MAX) {
      return t("hardwareConfig.validation.pinRange");
    }
    if ((HARDWARE_FORBIDDEN_PINS as readonly number[]).includes(pin)) {
      return t("hardwareConfig.validation.pinForbidden");
    }
    if (seenPins.has(pin)) {
      return t("hardwareConfig.validation.pinDup");
    }
    seenPins.add(pin);

    if (device.device_type === "adc_in" && pin > HARDWARE_ADC1_MAX_PIN) {
      return t("hardwareConfig.validation.adcPin");
    }
    if (device.device_type === "pwm_out") {
      pwmCount += 1;
      const hz = device.options?.frequency_hz;
      if (hz != null) {
        const normalized = typeof hz === "number" ? hz : Number(hz);
        if (
          !Number.isFinite(normalized) ||
          normalized < HARDWARE_PWM_FREQ_MIN ||
          normalized > HARDWARE_PWM_FREQ_MAX
        ) {
          return t("hardwareConfig.validation.pwmFreq");
        }
      }
    }
    if (device.device_type === "dht") {
      const model = device.options?.model;
      if (
        model != null &&
        typeof model === "string" &&
        !["dht11", "dht22", "dht21"].includes(model)
      ) {
        return t("hardwareConfig.validation.dhtModel");
      }
      const watchField = device.options?.watch_field;
      if (
        watchField != null &&
        typeof watchField === "string" &&
        watchField !== "temperature" &&
        watchField !== "humidity"
      ) {
        return t("hardwareConfig.validation.dhtWatchField");
      }
      const pull = device.options?.pull;
      if (
        pull != null &&
        typeof pull === "string" &&
        !["up", "down", "none"].includes(pull)
      ) {
        return t("hardwareConfig.validation.dhtPull");
      }
    }
  }

  if (pwmCount > MAX_PWM_DEVICES) {
    return t("hardwareConfig.validation.maxPwm");
  }

  const i2cBus = segment.i2c_bus ?? null;
  if (i2cBus != null) {
    if (
      !Number.isFinite(i2cBus.sda_pin) ||
      i2cBus.sda_pin < HARDWARE_PIN_MIN ||
      i2cBus.sda_pin > HARDWARE_PIN_MAX
    ) {
      return t("hardwareConfig.validation.i2cBusSdaPin");
    }
    if (
      !Number.isFinite(i2cBus.scl_pin) ||
      i2cBus.scl_pin < HARDWARE_PIN_MIN ||
      i2cBus.scl_pin > HARDWARE_PIN_MAX
    ) {
      return t("hardwareConfig.validation.i2cBusSclPin");
    }
    if (i2cBus.sda_pin === i2cBus.scl_pin) {
      return t("hardwareConfig.validation.i2cBusPinsDistinct");
    }
    if ((HARDWARE_FORBIDDEN_PINS as readonly number[]).includes(i2cBus.sda_pin)) {
      return t("hardwareConfig.validation.i2cBusSdaPin");
    }
    if ((HARDWARE_FORBIDDEN_PINS as readonly number[]).includes(i2cBus.scl_pin)) {
      return t("hardwareConfig.validation.i2cBusSclPin");
    }
    if (i2cBus.freq_hz != null) {
      const freq =
        typeof i2cBus.freq_hz === "number"
          ? i2cBus.freq_hz
          : Number(i2cBus.freq_hz);
      if (
        !Number.isFinite(freq) ||
        freq < I2C_BUS_FREQ_MIN ||
        freq > I2C_BUS_FREQ_MAX
      ) {
        return t("hardwareConfig.validation.i2cBusFreq");
      }
    }
  }

  const i2cSensors = segment.i2c_sensors ?? [];
  if (i2cSensors.length > MAX_I2C_SENSORS) {
    return t("hardwareConfig.validation.i2cSensorMax");
  }
  if (i2cSensors.length > 0 && i2cBus == null) {
    return t("hardwareConfig.validation.i2cBusRequired");
  }

  const seenI2cIds = new Set<string>();
  for (const sensor of i2cSensors) {
    if (!sensor.id.trim() || sensor.id.length > I2C_SENSOR_ID_MAX_LEN) {
      return t("hardwareConfig.validation.i2cSensorIdLen");
    }
    if (seenI2cIds.has(sensor.id)) {
      return t("hardwareConfig.validation.i2cSensorIdDup");
    }
    seenI2cIds.add(sensor.id);
    if (devices.some((device) => device.id === sensor.id)) {
      return t("hardwareConfig.validation.i2cSensorIdConflict");
    }
    if (
      !Number.isFinite(sensor.addr) ||
      sensor.addr < I2C_SENSOR_ADDR_MIN ||
      sensor.addr > I2C_SENSOR_ADDR_MAX
    ) {
      return t("hardwareConfig.validation.i2cSensorAddr");
    }
    if (!I2C_SENSOR_MODELS.includes(sensor.model as I2cSensorModel)) {
      return t("hardwareConfig.validation.i2cSensorModel");
    }
    if (
      sensor.watch_field !== "temperature" &&
      sensor.watch_field !== "humidity"
    ) {
      return t("hardwareConfig.validation.i2cSensorWatchField");
    }
    if (sensor.what.length > 128) {
      return t("hardwareConfig.validation.i2cSensorWhatLen");
    }
    if (sensor.how.length > 256) {
      return t("hardwareConfig.validation.i2cSensorHowLen");
    }

    if (sensor.model === "raw") {
      const init = sensor.options?.init_cmd;
      if (
        !Array.isArray(init) ||
        init.length === 0 ||
        init.length > I2C_SENSOR_MAX_CMD_LEN
      ) {
        return t("hardwareConfig.validation.i2cSensorRawInit");
      }
      for (const byte of init) {
        const normalized = typeof byte === "number" ? byte : Number(byte);
        if (!Number.isFinite(normalized) || normalized < 0 || normalized > 255) {
          return t("hardwareConfig.validation.i2cSensorRawInitByte");
        }
      }
      const readLenValue = sensor.options?.read_len;
      const readLen =
        typeof readLenValue === "number" ? readLenValue : Number(readLenValue);
      if (
        !Number.isFinite(readLen) ||
        readLen < 1 ||
        readLen > I2C_MAX_READ_LEN_UI
      ) {
        return t("hardwareConfig.validation.i2cSensorRawReadLen");
      }
      const waitValue = sensor.options?.conversion_wait_ms;
      if (waitValue != null) {
        const waitMs =
          typeof waitValue === "number" ? waitValue : Number(waitValue);
        if (!Number.isFinite(waitMs) || waitMs > 2000) {
          return t("hardwareConfig.validation.i2cSensorRawWait");
        }
      }
    }
  }

  return null;
}
