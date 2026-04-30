import { Suspense, lazy, useState } from 'react'
import Box from '@mui/material/Box'
import CircularProgress from '@mui/material/CircularProgress'
import { Routes, Route, Navigate } from 'react-router-dom'
import { ErrorBoundary } from './components/ErrorBoundary'
import { Layout } from './components/Layout'
import { ConfigProvider } from './contexts/ConfigProvider'
import { DeviceProvider } from './contexts/DeviceProvider'
import { ToastProvider } from './contexts/ToastProvider'
import { UnsavedProvider } from './contexts/UnsavedProvider'
import { useBlockNumberInputWheel } from './hooks/useBlockNumberInputWheel'
import { useScrollToTop } from './hooks/useScrollToTop'

const SettingsDrawer = lazy(async () => {
  const mod = await import('./components/SettingsDrawer')
  return { default: mod.SettingsDrawer }
})

const DevicePage = lazy(async () => {
  const mod = await import('./pages/DevicePage')
  return { default: mod.DevicePage }
})

const AIConfigPage = lazy(async () => {
  const mod = await import('./pages/AIConfigPage')
  return { default: mod.AIConfigPage }
})

const ChannelsConfigPage = lazy(async () => {
  const mod = await import('./pages/ChannelsConfigPage')
  return { default: mod.ChannelsConfigPage }
})

const SystemConfigPage = lazy(async () => {
  const mod = await import('./pages/SystemConfigPage')
  return { default: mod.SystemConfigPage }
})

const SystemLogsPage = lazy(async () => {
  const mod = await import('./pages/SystemLogsPage')
  return { default: mod.SystemLogsPage }
})

const SkillsPage = lazy(async () => {
  const mod = await import('./pages/SkillsPage')
  return { default: mod.SkillsPage }
})

const ToolsPage = lazy(async () => {
  const mod = await import('./pages/ToolsPage')
  return { default: mod.ToolsPage }
})

const AccountsPage = lazy(async () => {
  const mod = await import('./pages/AccountsPage')
  return { default: mod.AccountsPage }
})

const DeviceConfigLayout = lazy(async () => {
  const mod = await import('./pages/DeviceConfigLayout')
  return { default: mod.DeviceConfigLayout }
})

const DisplayConfigPanel = lazy(async () => {
  const mod = await import('./pages/DisplayConfigPanel')
  return { default: mod.DisplayConfigPanel }
})

const AudioConfigPanel = lazy(async () => {
  const mod = await import('./pages/AudioConfigPanel')
  return { default: mod.AudioConfigPanel }
})

const HardwareGpioPanel = lazy(async () => {
  const mod = await import('./pages/HardwareGpioPanel')
  return { default: mod.HardwareGpioPanel }
})

const I2cSensorsPanel = lazy(async () => {
  const mod = await import('./pages/I2cSensorsPanel')
  return { default: mod.I2cSensorsPanel }
})

const PlaceholderPage = lazy(async () => {
  const mod = await import('./pages/PlaceholderPage')
  return { default: mod.PlaceholderPage }
})

function RouteFallback() {
  return (
    <Box
      sx={{
        minHeight: '100vh',
        width: '100%',
        display: 'grid',
        placeItems: 'center',
        backgroundColor: 'var(--background)',
        backgroundImage: 'none',
      }}
    >
      <CircularProgress size={28} />
    </Box>
  )
}

function App() {
  useScrollToTop()
  useBlockNumberInputWheel()
  const [settingsOpen, setSettingsOpen] = useState(false)

  return (
    <DeviceProvider>
      <ToastProvider>
        <UnsavedProvider>
          <ConfigProvider>
            <ErrorBoundary>
              <Suspense fallback={null}>
                {settingsOpen ? (
                  <SettingsDrawer
                    open={settingsOpen}
                    onClose={() => setSettingsOpen(false)}
                  />
                ) : null}
              </Suspense>
              <Suspense fallback={<RouteFallback />}>
                <Routes>
                  <Route
                    element={<Layout onOpenSettings={() => setSettingsOpen(true)} />}
                  >
                    <Route path="/" element={<Navigate to="/device" replace />} />
                    <Route
                      path="/config"
                      element={<Navigate to="/ai-config" replace />}
                    />
                    <Route path="/device" element={<DevicePage />} />
                    <Route path="/ai-config" element={<AIConfigPage />} />
                    <Route
                      path="/channels-config"
                      element={<ChannelsConfigPage />}
                    />
                    <Route path="/system-config" element={<SystemConfigPage />} />
                    <Route
                      path="/device-config"
                      element={<DeviceConfigLayout />}
                    >
                      <Route index element={<Navigate to="display" replace />} />
                      <Route path="display" element={<DisplayConfigPanel />} />
                      <Route path="audio" element={<AudioConfigPanel />} />
                      <Route path="hardware" element={<HardwareGpioPanel />} />
                      <Route path="i2c-sensors" element={<I2cSensorsPanel />} />
                    </Route>
                    <Route
                      path="/display-config"
                      element={<Navigate to="/device-config/display" replace />}
                    />
                    <Route path="/system-logs" element={<SystemLogsPage />} />
                    <Route path="/skills" element={<SkillsPage />} />
                    <Route path="/tools" element={<ToolsPage />} />
                    <Route path="/accounts" element={<AccountsPage />} />
                    <Route
                      path="/soul-user"
                      element={<PlaceholderPage messageKey="common.pageRetired" />}
                    />
                    <Route path="*" element={<PlaceholderPage />} />
                  </Route>
                </Routes>
              </Suspense>
            </ErrorBoundary>
          </ConfigProvider>
        </UnsavedProvider>
      </ToastProvider>
    </DeviceProvider>
  )
}

export default App
