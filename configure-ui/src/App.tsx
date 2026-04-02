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

const SoulUserLayout = lazy(async () => {
  const mod = await import('./pages/soul-user')
  return { default: mod.SoulUserLayout }
})

const SoulUserSoulPanel = lazy(async () => {
  const mod = await import('./pages/soul-user')
  return { default: mod.SoulUserSoulPanel }
})

const SoulUserUserPanel = lazy(async () => {
  const mod = await import('./pages/soul-user')
  return { default: mod.SoulUserUserPanel }
})

const SkillsPage = lazy(async () => {
  const mod = await import('./pages/SkillsPage')
  return { default: mod.SkillsPage }
})

const ToolsPage = lazy(async () => {
  const mod = await import('./pages/ToolsPage')
  return { default: mod.ToolsPage }
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

const PlaceholderPage = lazy(async () => {
  const mod = await import('./pages/PlaceholderPage')
  return { default: mod.PlaceholderPage }
})

function RouteFallback() {
  return (
    <Box
      sx={{
        minHeight: 240,
        display: 'grid',
        placeItems: 'center',
      }}
    >
      <CircularProgress size={28} />
    </Box>
  )
}

function App() {
  useScrollToTop()
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
                    </Route>
                    <Route
                      path="/display-config"
                      element={<Navigate to="/device-config/display" replace />}
                    />
                    <Route path="/system-logs" element={<SystemLogsPage />} />
                    <Route path="/soul-user" element={<SoulUserLayout />}>
                      <Route index element={<Navigate to="soul" replace />} />
                      <Route path="soul" element={<SoulUserSoulPanel />} />
                      <Route path="user" element={<SoulUserUserPanel />} />
                    </Route>
                    <Route path="/skills" element={<SkillsPage />} />
                    <Route path="/tools" element={<ToolsPage />} />
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
