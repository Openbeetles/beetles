//! 硬件设备驱动函数（条件编译）。ESP 目标调用 esp-idf-svc，host 返回模拟值。
//! Hardware device driver functions (conditional compilation).

use crate::config::PinConfig;
use crate::error::{Error, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

// ── DHT sensor: rate limit (ESP + host stub) ──
/// DHT11 两次成功读数最小间隔（ms）。
const DHT11_MIN_INTERVAL_MS: u64 = 1_000;
/// DHT22/DHT21 两次成功读数最小间隔（ms）。
const DHT22_MIN_INTERVAL_MS: u64 = 2_000;

static DHT_LAST_READ: Mutex<Option<HashMap<i32, Instant>>> = Mutex::new(None);

fn dht_rate_limit_check(pin: i32, min_interval_ms: u64) -> Result<()> {
    let guard = DHT_LAST_READ.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(map) = guard.as_ref() {
        if let Some(prev) = map.get(&pin) {
            let elapsed_ms = prev.elapsed().as_millis() as u64;
            if elapsed_ms < min_interval_ms {
                return Err(Error::config(
                    "drive_dht",
                    format!(
                        "too frequent: wait {}ms before next read (min interval {}ms)",
                        min_interval_ms.saturating_sub(elapsed_ms),
                        min_interval_ms
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn dht_rate_limit_on_success(pin: i32) {
    let mut guard = DHT_LAST_READ.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(pin, Instant::now());
}

/// LEDC 定时器分辨率：13-bit (0–8191)。
const LEDC_DUTY_RESOLUTION_BITS: u32 = 13;
/// 13-bit 最大占空比值。
const LEDC_DUTY_MAX: u32 = (1 << LEDC_DUTY_RESOLUTION_BITS) - 1; // 8191
/// PWM 默认频率。
const PWM_DEFAULT_FREQ_HZ: u32 = 5000;
/// buzzer 最长响鸣时间（ms）。
const MAX_BUZZER_DURATION_MS: u64 = 3000;
/// buzzer 短鸣默认时长（ms）。
const BUZZER_BEEP_MS: u64 = 100;

// ── ESP32 target: real drivers ──

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn drive_gpio_out(pins: &PinConfig, params: &Value) -> Result<String> {
    use esp_idf_svc::sys::{
        gpio_config_t, gpio_get_level, gpio_int_type_t_GPIO_INTR_DISABLE,
        gpio_mode_t_GPIO_MODE_OUTPUT, gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
        gpio_pullup_t_GPIO_PULLUP_DISABLE, gpio_reset_pin, gpio_set_level, ESP_OK,
    };

    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("gpio_out", "missing pin"))?;
    let value = params
        .get("value")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::config("gpio_out", "missing or invalid 'value' (0 or 1)"))?;
    if value != 0 && value != 1 {
        return Err(Error::config("gpio_out", "value must be 0 or 1"));
    }

    unsafe {
        gpio_reset_pin(pin);
        let conf = gpio_config_t {
            pin_bit_mask: 1u64 << pin,
            mode: gpio_mode_t_GPIO_MODE_OUTPUT,
            pull_up_en: gpio_pullup_t_GPIO_PULLUP_DISABLE,
            pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
        };
        let ret = esp_idf_svc::sys::gpio_config(&conf);
        if ret != ESP_OK {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "gpio_config failed: {}",
                    ret
                ))),
                stage: "gpio_out",
            });
        }
        gpio_set_level(pin, if value != 0 { 1 } else { 0 });
        // read-back to confirm
        let actual = gpio_get_level(pin);
        let ok = actual == (if value != 0 { 1 } else { 0 });
        if ok {
            Ok(format!(r#"{{"ok":true,"actual_value":{}}}"#, actual))
        } else {
            Ok(format!(
                r#"{{"ok":false,"expected":{},"actual_value":{},"warning":"read-back mismatch"}}"#,
                value, actual
            ))
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn drive_gpio_in(pins: &PinConfig, _params: &Value, options: &Value) -> Result<String> {
    use esp_idf_svc::sys::{
        gpio_config_t, gpio_get_level, gpio_int_type_t_GPIO_INTR_DISABLE,
        gpio_mode_t_GPIO_MODE_INPUT, gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
        gpio_pulldown_t_GPIO_PULLDOWN_ENABLE, gpio_pullup_t_GPIO_PULLUP_DISABLE,
        gpio_pullup_t_GPIO_PULLUP_ENABLE, gpio_reset_pin, ESP_OK,
    };

    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("gpio_in", "missing pin"))?;
    let pull = options
        .get("pull")
        .and_then(|v| v.as_str())
        .unwrap_or("none");

    let (pull_up, pull_down) = match pull {
        "up" => (
            gpio_pullup_t_GPIO_PULLUP_ENABLE,
            gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
        ),
        "down" => (
            gpio_pullup_t_GPIO_PULLUP_DISABLE,
            gpio_pulldown_t_GPIO_PULLDOWN_ENABLE,
        ),
        _ => (
            gpio_pullup_t_GPIO_PULLUP_DISABLE,
            gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
        ),
    };

    unsafe {
        gpio_reset_pin(pin);
        let conf = gpio_config_t {
            pin_bit_mask: 1u64 << pin,
            mode: gpio_mode_t_GPIO_MODE_INPUT,
            pull_up_en: pull_up,
            pull_down_en: pull_down,
            intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
        };
        let ret = esp_idf_svc::sys::gpio_config(&conf);
        if ret != ESP_OK {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "gpio_config failed: {}",
                    ret
                ))),
                stage: "gpio_in",
            });
        }
        let level = gpio_get_level(pin);
        Ok(format!(r#"{{"value":{}}}"#, level))
    }
}

/// LEDC timer index to timer enum (ESP32-S3 has 4 timers 0..3; each pwm_out gets its own for independent frequency).
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn ledc_timer_from_index(i: u8) -> esp_idf_svc::sys::ledc_timer_t {
    // C enum LEDC_TIMER_0=0 .. LEDC_TIMER_3=3; repr(C) enum is typically 4 bytes.
    i.min(3) as u32
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn drive_pwm_out(
    pins: &PinConfig,
    params: &Value,
    options: &Value,
    ledc_channel: u8,
    ledc_timer_index: u8,
) -> Result<String> {
    use esp_idf_svc::sys::{
        ledc_channel_config, ledc_channel_config_t, ledc_intr_type_t_LEDC_INTR_DISABLE,
        ledc_mode_t_LEDC_LOW_SPEED_MODE, ledc_set_duty, ledc_timer_bit_t_LEDC_TIMER_13_BIT,
        ledc_timer_config, ledc_timer_config_t, ledc_update_duty, ESP_OK,
    };

    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("pwm_out", "missing pin"))?;
    let duty = params
        .get("duty")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| Error::config("pwm_out", "missing or invalid 'duty' (0–100)"))?;
    if duty > 100 {
        return Err(Error::config("pwm_out", "duty must be 0–100"));
    }
    let freq_hz = options
        .get("frequency_hz")
        .and_then(|v| v.as_u64())
        .unwrap_or(PWM_DEFAULT_FREQ_HZ as u64) as u32;

    // Map 0–100 to 0–8191
    let duty_raw = (duty as u32 * LEDC_DUTY_MAX) / 100;
    let speed_mode = ledc_mode_t_LEDC_LOW_SPEED_MODE;
    let channel = ledc_channel as u32;
    let timer_sel = ledc_timer_from_index(ledc_timer_index);

    unsafe {
        // Configure this device's timer (one timer per pwm_out so frequency is independent).
        let timer_conf = ledc_timer_config_t {
            speed_mode,
            duty_resolution: ledc_timer_bit_t_LEDC_TIMER_13_BIT,
            timer_num: timer_sel,
            freq_hz,
            clk_cfg: esp_idf_svc::sys::soc_periph_ledc_clk_src_legacy_t_LEDC_AUTO_CLK,
            deconfigure: false,
        };
        let ret = ledc_timer_config(&timer_conf);
        if ret != ESP_OK {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "ledc_timer_config failed: {}",
                    ret
                ))),
                stage: "pwm_out",
            });
        }

        // Configure channel + bind to GPIO and this timer
        let ch_conf = ledc_channel_config_t {
            speed_mode,
            channel,
            timer_sel,
            intr_type: ledc_intr_type_t_LEDC_INTR_DISABLE,
            gpio_num: pin,
            duty: duty_raw,
            hpoint: 0,
            flags: Default::default(),
            ..Default::default()
        };
        let ret = ledc_channel_config(&ch_conf);
        if ret != ESP_OK {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "ledc_channel_config failed: {}",
                    ret
                ))),
                stage: "pwm_out",
            });
        }

        ledc_set_duty(speed_mode, channel, duty_raw);
        ledc_update_duty(speed_mode, channel);
    }

    Ok(format!(
        r#"{{"ok":true,"duty":{},"duty_raw":{}}}"#,
        duty, duty_raw
    ))
}

/// ADC1 使用 oneshot 驱动（esp_adc/adc_oneshot.h），替代已弃用的 driver/adc.h legacy API。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn drive_adc_in(pins: &PinConfig, _params: &Value, options: &Value) -> Result<String> {
    #[cfg(not(esp_idf_version_at_least_6_0_0))]
    use esp_idf_svc::sys::adc_atten_t_ADC_ATTEN_DB_11;
    #[cfg(esp_idf_version_at_least_6_0_0)]
    use esp_idf_svc::sys::adc_atten_t_ADC_ATTEN_DB_12;
    use esp_idf_svc::sys::{
        adc_atten_t_ADC_ATTEN_DB_0, adc_atten_t_ADC_ATTEN_DB_2_5, adc_atten_t_ADC_ATTEN_DB_6,
        adc_bitwidth_t_ADC_BITWIDTH_12, adc_oneshot_chan_cfg_t, adc_oneshot_config_channel,
        adc_oneshot_del_unit, adc_oneshot_new_unit, adc_oneshot_read, adc_oneshot_unit_init_cfg_t,
        adc_ulp_mode_t_ADC_ULP_MODE_DISABLE, adc_unit_t_ADC_UNIT_1,
        soc_periph_adc_rtc_clk_src_t_ADC_RTC_CLK_SRC_DEFAULT, ESP_OK,
    };

    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("adc_in", "missing pin"))?;

    // ESP32-S3: ADC1 channels map GPIO 1–10 → channel 0–9
    if !(1..=10).contains(&pin) {
        return Err(Error::config(
            "adc_in",
            format!("pin {} not in ADC1 range (GPIO 1–10)", pin),
        ));
    }
    let channel = (pin - 1) as u32;

    let atten_str = options
        .get("atten")
        .and_then(|v| v.as_str())
        .unwrap_or("11db");
    let atten = match atten_str {
        "0db" => adc_atten_t_ADC_ATTEN_DB_0,
        "2.5db" => adc_atten_t_ADC_ATTEN_DB_2_5,
        "6db" => adc_atten_t_ADC_ATTEN_DB_6,
        // IDF 6: ADC_ATTEN_DB_11 renamed to ADC_ATTEN_DB_12 (same ~11 dB range).
        #[cfg(esp_idf_version_at_least_6_0_0)]
        _ => adc_atten_t_ADC_ATTEN_DB_12,
        #[cfg(not(esp_idf_version_at_least_6_0_0))]
        _ => adc_atten_t_ADC_ATTEN_DB_11,
    };

    unsafe {
        let init_cfg = adc_oneshot_unit_init_cfg_t {
            unit_id: adc_unit_t_ADC_UNIT_1,
            clk_src: soc_periph_adc_rtc_clk_src_t_ADC_RTC_CLK_SRC_DEFAULT,
            ulp_mode: adc_ulp_mode_t_ADC_ULP_MODE_DISABLE,
        };
        let mut handle = core::ptr::null_mut();
        let ret = adc_oneshot_new_unit(&init_cfg, &mut handle);
        if ret != ESP_OK {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "adc_oneshot_new_unit failed: {}",
                    ret
                ))),
                stage: "adc_in",
            });
        }
        let chan_cfg = adc_oneshot_chan_cfg_t {
            atten,
            bitwidth: adc_bitwidth_t_ADC_BITWIDTH_12,
        };
        let ret = adc_oneshot_config_channel(handle, channel, &chan_cfg);
        if ret != ESP_OK {
            let _ = adc_oneshot_del_unit(handle);
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "adc_oneshot_config_channel failed: {}",
                    ret
                ))),
                stage: "adc_in",
            });
        }
        let mut raw: i32 = 0;
        let ret = adc_oneshot_read(handle, channel, &mut raw);
        let _ = adc_oneshot_del_unit(handle);
        if ret != ESP_OK {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "adc_oneshot_read failed: {}",
                    ret
                ))),
                stage: "adc_in",
            });
        }
        Ok(format!(r#"{{"raw":{}}}"#, raw))
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn drive_buzzer(pins: &PinConfig, params: &Value) -> Result<String> {
    use esp_idf_svc::sys::{
        gpio_config_t, gpio_int_type_t_GPIO_INTR_DISABLE, gpio_mode_t_GPIO_MODE_OUTPUT,
        gpio_pulldown_t_GPIO_PULLDOWN_DISABLE, gpio_pullup_t_GPIO_PULLUP_DISABLE, gpio_reset_pin,
        gpio_set_level, ESP_OK,
    };

    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("buzzer", "missing pin"))?;

    // Determine duration: beep=true → 100ms, or duration_ms (clamped to MAX)
    let beep = params
        .get("beep")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut duration_ms = if beep {
        BUZZER_BEEP_MS
    } else {
        params
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(BUZZER_BEEP_MS)
    };
    let clamped = duration_ms > MAX_BUZZER_DURATION_MS;
    if clamped {
        duration_ms = MAX_BUZZER_DURATION_MS;
    }

    unsafe {
        gpio_reset_pin(pin);
        let conf = gpio_config_t {
            pin_bit_mask: 1u64 << pin,
            mode: gpio_mode_t_GPIO_MODE_OUTPUT,
            pull_up_en: gpio_pullup_t_GPIO_PULLUP_DISABLE,
            pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
        };
        let ret = esp_idf_svc::sys::gpio_config(&conf);
        if ret != ESP_OK {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "gpio_config failed: {}",
                    ret
                ))),
                stage: "buzzer",
            });
        }
        // Turn on
        gpio_set_level(pin, 1);
    }

    // Non-blocking: spawn a thread to turn off after duration
    let dur = std::time::Duration::from_millis(duration_ms);
    crate::util::spawn_guarded_with_profile_handle(
        "buzzer_off",
        2048,
        Some(crate::util::SpawnCore::Core1),
        crate::util::HttpThreadRole::Background,
        move || {
            std::thread::sleep(dur);
            unsafe {
                esp_idf_svc::sys::gpio_set_level(pin, 0);
            }
        },
    )
    .map_err(|e| Error::Other {
        source: Box::new(e),
        stage: "buzzer",
    })?;

    if clamped {
        Ok(format!(
            r#"{{"ok":true,"duration_ms":{},"warning":"clamped to max {}ms"}}"#,
            duration_ms, MAX_BUZZER_DURATION_MS
        ))
    } else {
        Ok(format!(r#"{{"ok":true,"duration_ms":{}}}"#, duration_ms))
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::time::Duration;

/// DHT11 启动拉低时间（μs）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT11_START_LOW_US: u32 = 18_000;
/// DHT22/DHT21 启动拉低时间（μs）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT22_START_LOW_US: u32 = 1_000;
/// 启动后释放总线再等待（μs）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT_START_RELEASE_US: u32 = 30;
/// 释放总线后等待从机首次拉低的最长时间（µs）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT_WAIT_FIRST_FALL_US: i64 = 5_000;
/// 应答阶段单边沿最长等待（µs）（手册约 80µs）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT_ACK_EDGE_TIMEOUT_US: i64 = 600;
/// 数据位：低电平结束（上升沿）最长等待（µs）（部分模块低相可略超 50µs）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT_BIT_LOW_END_TIMEOUT_US: i64 = 600;
/// 上升沿后再延时该时间（µs）后读电平判 0/1（0 位高相 ~26–28µs，1 位 ~70µs；与常见 Arduino 库 40µs 一致）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT_BIT_SAMPLE_DELAY_US: u32 = 40;
/// 采样后等待高电平结束（下降沿）的最长时间（µs），用于与下一位起始低电平对齐；连续 1 时必需。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT_BIT_WAIT_FALL_US: i64 = 2_000;
/// 失败后重试次数（不含首次）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT_MAX_RETRIES: u8 = 3;
/// 重试间隔（ms）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const DHT_RETRY_DELAY_MS: u64 = 150;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
static DHT_SAMPLE_CRIT: esp_idf_hal::interrupt::IsrCriticalSection =
    esp_idf_hal::interrupt::IsrCriticalSection::new();

/// 等待 `gpio_get_level(pin) == target`，超时返回 `Err`。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
unsafe fn dht_wait_until_level(
    pin: i32,
    target: i32,
    timeout_us: i64,
    err: &'static str,
) -> Result<()> {
    use esp_idf_svc::sys::{esp_timer_get_time, gpio_get_level};

    let deadline = esp_timer_get_time().saturating_add(timeout_us);
    while gpio_get_level(pin) != target {
        if esp_timer_get_time() > deadline {
            return Err(Error::config("drive_dht", err));
        }
    }
    Ok(())
}

/// DHT 单总线采样：握手后读 40 位，返回 5 字节原始帧。
/// 数据位：上升沿后 `esp_rom_delay_us` 再读电平（与常见 DHT 库一致），随后等下降沿再读下一位，避免「只测高脉宽」在 S3 上偶发等不到下降沿。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
unsafe fn dht_sample_raw_frame(pin: i32) -> Result<[u8; 5]> {
    use esp_idf_svc::sys::{esp_rom_delay_us, gpio_get_level};

    let _g = DHT_SAMPLE_CRIT.enter();

    dht_wait_until_level(
        pin,
        0,
        DHT_WAIT_FIRST_FALL_US,
        "timeout waiting for sensor response (high→low)",
    )?;
    dht_wait_until_level(
        pin,
        1,
        DHT_ACK_EDGE_TIMEOUT_US,
        "timeout during sensor ack (low phase)",
    )?;
    dht_wait_until_level(
        pin,
        0,
        DHT_ACK_EDGE_TIMEOUT_US,
        "timeout during sensor ack (high phase)",
    )?;

    let mut data = [0u8; 5];
    for i in 0..40u32 {
        dht_wait_until_level(
            pin,
            1,
            DHT_BIT_LOW_END_TIMEOUT_US,
            "timeout waiting for data bit low→high",
        )?;
        esp_rom_delay_us(DHT_BIT_SAMPLE_DELAY_US);
        if gpio_get_level(pin) != 0 {
            data[(i / 8) as usize] |= 1 << (7 - (i % 8));
        }
        dht_wait_until_level(
            pin,
            0,
            DHT_BIT_WAIT_FALL_US,
            "timeout during data bit high phase",
        )?;
    }

    Ok(data)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn dht_pull_from_options(
    options: &Value,
) -> (
    esp_idf_svc::sys::gpio_pullup_t,
    esp_idf_svc::sys::gpio_pulldown_t,
) {
    use esp_idf_svc::sys::{
        gpio_pulldown_t_GPIO_PULLDOWN_DISABLE, gpio_pulldown_t_GPIO_PULLDOWN_ENABLE,
        gpio_pullup_t_GPIO_PULLUP_DISABLE, gpio_pullup_t_GPIO_PULLUP_ENABLE,
    };
    let pull = options.get("pull").and_then(|v| v.as_str()).unwrap_or("up");
    match pull {
        "down" => (
            gpio_pullup_t_GPIO_PULLUP_DISABLE,
            gpio_pulldown_t_GPIO_PULLDOWN_ENABLE,
        ),
        "none" => (
            gpio_pullup_t_GPIO_PULLUP_DISABLE,
            gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
        ),
        _ => (
            gpio_pullup_t_GPIO_PULLUP_ENABLE,
            gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
        ),
    }
}

/// DHT11/DHT22/DHT21 单总线读取；JSON 与 `drive_gpio_in` / `drive_adc_in` 风格一致。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn drive_dht(pins: &PinConfig, _params: &Value, options: &Value) -> Result<String> {
    use esp_idf_svc::sys::{
        esp_rom_delay_us, gpio_config, gpio_config_t, gpio_int_type_t_GPIO_INTR_DISABLE,
        gpio_mode_t_GPIO_MODE_INPUT, gpio_mode_t_GPIO_MODE_OUTPUT, gpio_reset_pin, gpio_set_level,
        ESP_OK,
    };

    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("drive_dht", "missing pin"))?;

    let model = options
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("dht11");
    if model != "dht11" && model != "dht22" && model != "dht21" {
        return Err(Error::config(
            "drive_dht",
            format!("options.model must be dht11|dht22|dht21, got '{}'", model),
        ));
    }

    let (start_low_us, min_interval_ms) = if model == "dht11" {
        (DHT11_START_LOW_US, DHT11_MIN_INTERVAL_MS)
    } else {
        (DHT22_START_LOW_US, DHT22_MIN_INTERVAL_MS)
    };

    dht_rate_limit_check(pin, min_interval_ms)?;

    let (pull_up, pull_down) = dht_pull_from_options(options);
    let mut last_err: Option<Error> = None;

    for attempt in 0..=DHT_MAX_RETRIES {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(DHT_RETRY_DELAY_MS));
        }

        let sample_result: Result<[u8; 5]> = (|| unsafe {
            gpio_reset_pin(pin);
            let conf_out = gpio_config_t {
                pin_bit_mask: 1u64 << pin,
                mode: gpio_mode_t_GPIO_MODE_OUTPUT,
                pull_up_en: pull_up,
                pull_down_en: pull_down,
                intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
            };
            let ret = gpio_config(&conf_out);
            if ret != ESP_OK {
                return Err(Error::Other {
                    source: Box::new(std::io::Error::other(format!(
                        "gpio_config output failed: {}",
                        ret
                    ))),
                    stage: "drive_dht",
                });
            }

            gpio_set_level(pin, 0);
            esp_rom_delay_us(start_low_us);
            gpio_set_level(pin, 1);
            esp_rom_delay_us(DHT_START_RELEASE_US);

            let conf_in = gpio_config_t {
                pin_bit_mask: 1u64 << pin,
                mode: gpio_mode_t_GPIO_MODE_INPUT,
                pull_up_en: pull_up,
                pull_down_en: pull_down,
                intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
            };
            let ret = gpio_config(&conf_in);
            if ret != ESP_OK {
                return Err(Error::Other {
                    source: Box::new(std::io::Error::other(format!(
                        "gpio_config input failed: {}",
                        ret
                    ))),
                    stage: "drive_dht",
                });
            }

            dht_sample_raw_frame(pin)
        })();

        let data = match sample_result {
            Ok(d) => d,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };

        let sum = data[0]
            .wrapping_add(data[1])
            .wrapping_add(data[2])
            .wrapping_add(data[3]);
        if (sum & 0xFF) != data[4] {
            log::warn!(
                "[drive_dht] checksum mismatch: [{:#04x},{:#04x},{:#04x},{:#04x},{:#04x}] sum={:#04x} attempt={}",
                data[0],
                data[1],
                data[2],
                data[3],
                data[4],
                sum & 0xFF,
                attempt
            );
            last_err = Some(Error::config("drive_dht", "checksum mismatch"));
            continue;
        }

        let (temperature, humidity) = if model == "dht11" {
            let h = f64::from(data[0]);
            let t = f64::from(data[2]);
            (t, h)
        } else {
            let rh_raw = u16::from_be_bytes([data[0], data[1]]);
            let t_raw = u16::from_be_bytes([data[2], data[3]]);
            let h = f64::from(rh_raw) / 10.0;
            let t = if (t_raw & 0x8000) != 0 {
                -f64::from(t_raw & 0x7FFF) / 10.0
            } else {
                f64::from(t_raw) / 10.0
            };
            (t, h)
        };

        dht_rate_limit_on_success(pin);
        return Ok(format!(
            r#"{{"temperature":{},"humidity":{},"model":"{}"}}"#,
            temperature, humidity, model
        ));
    }

    Err(last_err.unwrap_or_else(|| Error::config("drive_dht", "read failed")))
}

// ── Host target: stub drivers for cargo check / clippy ──

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_gpio_out(pins: &PinConfig, params: &Value) -> Result<String> {
    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("gpio_out", "missing pin"))?;
    let value = params
        .get("value")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::config("gpio_out", "missing or invalid 'value' (0 or 1)"))?;
    if value != 0 && value != 1 {
        return Err(Error::config("gpio_out", "value must be 0 or 1"));
    }
    log::info!("[gpio_out] stub: pin={} value={}", pin, value);
    Ok(format!(
        r#"{{"ok":true,"actual_value":{},"stub":true}}"#,
        value
    ))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_gpio_in(pins: &PinConfig, _params: &Value, options: &Value) -> Result<String> {
    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("gpio_in", "missing pin"))?;
    let _pull = options
        .get("pull")
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    log::info!("[gpio_in] stub: pin={}", pin);
    Ok(r#"{"value":0,"stub":true}"#.to_string())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_pwm_out(
    pins: &PinConfig,
    params: &Value,
    options: &Value,
    ledc_channel: u8,
    ledc_timer_index: u8,
) -> Result<String> {
    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("pwm_out", "missing pin"))?;
    let duty = params
        .get("duty")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| Error::config("pwm_out", "missing or invalid 'duty' (0–100)"))?;
    if duty > 100 {
        return Err(Error::config("pwm_out", "duty must be 0–100"));
    }
    let freq = options
        .get("frequency_hz")
        .and_then(|v| v.as_u64())
        .unwrap_or(PWM_DEFAULT_FREQ_HZ as u64);
    let duty_raw = (duty as u32 * LEDC_DUTY_MAX) / 100;
    log::info!(
        "[pwm_out] stub: pin={} ch={} timer={} duty={}% raw={} freq={}",
        pin,
        ledc_channel,
        ledc_timer_index,
        duty,
        duty_raw,
        freq
    );
    Ok(format!(
        r#"{{"ok":true,"duty":{},"duty_raw":{},"stub":true}}"#,
        duty, duty_raw
    ))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_adc_in(pins: &PinConfig, _params: &Value, options: &Value) -> Result<String> {
    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("adc_in", "missing pin"))?;
    if !(1..=10).contains(&pin) {
        return Err(Error::config(
            "adc_in",
            format!("pin {} not in ADC1 range (GPIO 1–10)", pin),
        ));
    }
    let _atten = options
        .get("atten")
        .and_then(|v| v.as_str())
        .unwrap_or("11db");
    log::info!("[adc_in] stub: pin={}", pin);
    Ok(r#"{"raw":2048,"stub":true}"#.to_string())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_buzzer(pins: &PinConfig, params: &Value) -> Result<String> {
    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("buzzer", "missing pin"))?;
    let beep = params
        .get("beep")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut duration_ms = if beep {
        BUZZER_BEEP_MS
    } else {
        params
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(BUZZER_BEEP_MS)
    };
    let clamped = duration_ms > MAX_BUZZER_DURATION_MS;
    if clamped {
        duration_ms = MAX_BUZZER_DURATION_MS;
    }
    log::info!(
        "[buzzer] stub: pin={} duration_ms={} clamped={}",
        pin,
        duration_ms,
        clamped
    );
    if clamped {
        Ok(format!(
            r#"{{"ok":true,"duration_ms":{},"warning":"clamped to max {}ms","stub":true}}"#,
            duration_ms, MAX_BUZZER_DURATION_MS
        ))
    } else {
        Ok(format!(
            r#"{{"ok":true,"duration_ms":{},"stub":true}}"#,
            duration_ms
        ))
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_dht(pins: &PinConfig, _params: &Value, options: &Value) -> Result<String> {
    let pin = *pins
        .get("pin")
        .ok_or_else(|| Error::config("drive_dht", "missing pin"))?;
    let model = options
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("dht11");
    if model != "dht11" && model != "dht22" && model != "dht21" {
        return Err(Error::config(
            "drive_dht",
            format!("options.model must be dht11|dht22|dht21, got '{}'", model),
        ));
    }
    let min_interval_ms = if model == "dht11" {
        DHT11_MIN_INTERVAL_MS
    } else {
        DHT22_MIN_INTERVAL_MS
    };
    dht_rate_limit_check(pin, min_interval_ms)?;
    log::info!("[drive_dht] stub: pin={} model={}", pin, model);
    dht_rate_limit_on_success(pin);
    Ok(format!(
        r#"{{"temperature":22.0,"humidity":55.0,"model":"{}","stub":true}}"#,
        model
    ))
}

// ── I2C drivers (ESP-IDF `driver/i2c_master.h`, IDF 5.4+) ──

/// I2C master 总线状态：bus handle + 按 7 位地址缓存的 device handle。
/// I2C master bus state: bus handle and per-address device handles (lazy).
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) struct I2cBusState {
    bus: esp_idf_svc::sys::i2c_master_bus_handle_t,
    devices: std::collections::HashMap<u8, esp_idf_svc::sys::i2c_master_dev_handle_t>,
    freq_hz: u32,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
unsafe impl Send for I2cBusState {}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl I2cBusState {
    /// 使用配置的 SDA/SCL/频率创建 I2C master bus（端口 I2C_NUM_0）。
    pub(crate) fn new(sda: i32, scl: i32, freq_hz: u32) -> Result<Self> {
        use esp_idf_svc::sys::{i2c_new_master_bus, ESP_OK};

        let mut bus_config: esp_idf_svc::sys::i2c_master_bus_config_t =
            unsafe { core::mem::zeroed() };
        // `i2c_port_num_t`: I2C_NUM_0；全零的 clk_source union 为默认时钟源。
        bus_config.i2c_port = 0;
        bus_config.sda_io_num = sda;
        bus_config.scl_io_num = scl;
        bus_config.glitch_ignore_cnt = 7;
        bus_config.intr_priority = 0;
        bus_config.trans_queue_depth = 0;

        let mut bus: esp_idf_svc::sys::i2c_master_bus_handle_t = core::ptr::null_mut();
        let ret = unsafe { i2c_new_master_bus(&bus_config, &mut bus) };
        if ret != ESP_OK {
            return Err(Error::esp("i2c_init", ret));
        }
        Ok(Self {
            bus,
            devices: std::collections::HashMap::new(),
            freq_hz,
        })
    }

    fn ensure_device(&mut self, addr: u8) -> Result<esp_idf_svc::sys::i2c_master_dev_handle_t> {
        use esp_idf_svc::sys::{
            i2c_addr_bit_len_t_I2C_ADDR_BIT_LEN_7, i2c_master_bus_add_device, ESP_OK,
        };

        if let Some(&h) = self.devices.get(&addr) {
            return Ok(h);
        }
        let mut dev_cfg: esp_idf_svc::sys::i2c_device_config_t = unsafe { core::mem::zeroed() };
        dev_cfg.dev_addr_length = i2c_addr_bit_len_t_I2C_ADDR_BIT_LEN_7;
        dev_cfg.device_address = u16::from(addr);
        dev_cfg.scl_speed_hz = self.freq_hz;

        let mut dev: esp_idf_svc::sys::i2c_master_dev_handle_t = core::ptr::null_mut();
        let ret = unsafe { i2c_master_bus_add_device(self.bus, &dev_cfg, &mut dev) };
        if ret != ESP_OK {
            return Err(Error::esp("i2c_add_device", ret));
        }
        self.devices.insert(addr, dev);
        Ok(dev)
    }

    /// 寄存器读：写寄存器地址后 repeated start 再读（`i2c_master_transmit_receive`）。
    pub(crate) fn read(&mut self, addr: u8, register: u8, len: usize) -> Result<Vec<u8>> {
        use esp_idf_svc::sys::{i2c_master_transmit_receive, ESP_OK};

        let dev = self.ensure_device(addr)?;
        let write_buf = [register];
        let mut read_buf = vec![0u8; len];
        let ret = unsafe {
            i2c_master_transmit_receive(
                dev,
                write_buf.as_ptr(),
                write_buf.len(),
                read_buf.as_mut_ptr(),
                read_buf.len(),
                -1,
            )
        };
        if ret != ESP_OK {
            return Err(Error::esp("i2c_read", ret));
        }
        Ok(read_buf)
    }

    /// 寄存器写：单帧发送 `[register, ...data]`。
    pub(crate) fn write(&mut self, addr: u8, register: u8, data: &[u8]) -> Result<()> {
        use esp_idf_svc::sys::{i2c_master_transmit, ESP_OK};

        let dev = self.ensure_device(addr)?;
        let mut buf = Vec::with_capacity(1 + data.len());
        buf.push(register);
        buf.extend_from_slice(data);
        let ret = unsafe { i2c_master_transmit(dev, buf.as_ptr(), buf.len(), -1) };
        if ret != ESP_OK {
            return Err(Error::esp("i2c_write", ret));
        }
        Ok(())
    }

    /// 纯读：无寄存器前缀（`i2c_master_receive`），用于 SHT3x/AHT20 等测量后读回。
    pub(crate) fn receive(&mut self, addr: u8, len: usize) -> Result<Vec<u8>> {
        use esp_idf_svc::sys::{i2c_master_receive, ESP_OK};

        let dev = self.ensure_device(addr)?;
        let mut read_buf = vec![0u8; len];
        let ret = unsafe { i2c_master_receive(dev, read_buf.as_mut_ptr(), read_buf.len(), -1) };
        if ret != ESP_OK {
            return Err(Error::esp("i2c_read", ret));
        }
        Ok(read_buf)
    }
}

/// SHT3x CRC-8：多项式 0x31，初值 0xFF。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn crc8_sht3x(data: &[u8]) -> u8 {
    let mut crc = 0xFFu8;
    for b in data {
        crc ^= *b;
        for _ in 0..8 {
            if (crc & 0x80) != 0 {
                crc = (crc << 1) ^ 0x31;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// 解析 SHT3x 单次测量 6 字节帧（湿度 + CRC + 温度 + CRC）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn parse_sht3x(data: &[u8]) -> Result<(f64, f64)> {
    if data.len() < 6 {
        return Err(Error::config(
            "drive_i2c_sensor",
            format!("sht3x expected 6 bytes, got {}", data.len()),
        ));
    }
    if crc8_sht3x(&data[0..2]) != data[2] {
        return Err(Error::config(
            "drive_i2c_sensor",
            "sht3x humidity CRC mismatch",
        ));
    }
    if crc8_sht3x(&data[3..5]) != data[5] {
        return Err(Error::config(
            "drive_i2c_sensor",
            "sht3x temperature CRC mismatch",
        ));
    }
    let rh_raw = u16::from_be_bytes([data[0], data[1]]);
    let t_raw = u16::from_be_bytes([data[3], data[4]]);
    let humidity = 100.0 * f64::from(rh_raw) / 65535.0;
    let temperature = -45.0 + 175.0 * f64::from(t_raw) / 65535.0;
    Ok((temperature, humidity))
}

/// 解析 AHT20 测量 6 字节帧（状态 + 20bit 湿度 + 20bit 温度）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) fn parse_aht20(data: &[u8]) -> Result<(f64, f64)> {
    if data.len() < 6 {
        return Err(Error::config(
            "drive_i2c_sensor",
            format!("aht20 expected 6 bytes, got {}", data.len()),
        ));
    }
    if (data[0] & 0x80) != 0 {
        return Err(Error::config(
            "drive_i2c_sensor",
            "aht20 sensor busy (status bit 7 set)",
        ));
    }
    let h_raw: u32 =
        ((u32::from(data[1]) << 12) | (u32::from(data[2]) << 4) | (u32::from(data[3]) >> 4))
            & 0xFFFFF;
    let t_raw: u32 =
        (((u32::from(data[3]) & 0x0F) << 16) | (u32::from(data[4]) << 8) | u32::from(data[5]))
            & 0xFFFFF;
    let humidity = (h_raw as f64) * 100.0 / f64::from(1u32 << 20);
    let temperature = (t_raw as f64) * 200.0 / f64::from(1u32 << 20) - 50.0;
    Ok((temperature, humidity))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Drop for I2cBusState {
    fn drop(&mut self) {
        use esp_idf_svc::sys::{i2c_del_master_bus, i2c_master_bus_rm_device};

        for (_, h) in self.devices.drain() {
            unsafe {
                let _ = i2c_master_bus_rm_device(h);
            }
        }
        if !self.bus.is_null() {
            unsafe {
                let _ = i2c_del_master_bus(self.bus);
            }
        }
    }
}

/// I2C 读取：Host stub。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_i2c_read(addr: u8, register: u8, len: usize) -> Result<Vec<u8>> {
    log::info!(
        "[i2c_read] stub: addr=0x{:02X} reg=0x{:02X} len={}",
        addr,
        register,
        len
    );
    Ok(vec![0u8; len])
}

/// I2C 写入：Host stub。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_i2c_write(addr: u8, register: u8, data: &[u8]) -> Result<()> {
    log::info!(
        "[i2c_write] stub: addr=0x{:02X} reg=0x{:02X} data={:?}",
        addr,
        register,
        data
    );
    Ok(())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
static I2C_SENSOR_LAST_READ: Mutex<Option<HashMap<i32, Instant>>> = Mutex::new(None);

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn i2c_sensor_stub_rate_limit_check(addr: u8) -> Result<()> {
    use crate::constants::I2C_SENSOR_RATE_LIMIT_MS;
    let pin_key = i32::from(addr);
    let guard = I2C_SENSOR_LAST_READ
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(map) = guard.as_ref() {
        if let Some(prev) = map.get(&pin_key) {
            let elapsed_ms = prev.elapsed().as_millis() as u64;
            if elapsed_ms < I2C_SENSOR_RATE_LIMIT_MS {
                return Err(Error::config(
                    "drive_i2c_sensor",
                    format!(
                        "too frequent: wait {}ms (min interval {}ms)",
                        I2C_SENSOR_RATE_LIMIT_MS.saturating_sub(elapsed_ms),
                        I2C_SENSOR_RATE_LIMIT_MS
                    ),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn i2c_sensor_stub_rate_limit_on_success(addr: u8) {
    let pin_key = i32::from(addr);
    let mut guard = I2C_SENSOR_LAST_READ
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(pin_key, Instant::now());
}

/// Host：`drive_i2c_sensor` 模拟 JSON（含速率限制）。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub fn drive_i2c_sensor_stub(addr: u8, model: &str) -> Result<String> {
    i2c_sensor_stub_rate_limit_check(addr)?;
    log::info!(
        "[drive_i2c_sensor] stub: addr=0x{:02X} model={}",
        addr,
        model
    );
    i2c_sensor_stub_rate_limit_on_success(addr);
    if model == "raw" {
        Ok(r#"{"raw":"000000000000","model":"raw","stub":true}"#.to_string())
    } else {
        Ok(format!(
            r#"{{"temperature":22.0,"humidity":55.0,"model":"{}","stub":true}}"#,
            model
        ))
    }
}
