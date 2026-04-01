import { useState } from 'react'
import { Routes, Route, Navigate } from 'react-router-dom'
import { ErrorBoundary } from './components/ErrorBoundary'
import { Layout } from './components/Layout'
import { SettingsDrawer } from './components/SettingsDrawer'
import { ConfigProvider } from './contexts/ConfigProvider'
import { DeviceProvider } from './contexts/DeviceProvider'
import { ToastProvider } from './contexts/ToastProvider'
import { UnsavedProvider } from './contexts/UnsavedProvider'
import { DevicePage } from './pages/DevicePage'
import { AIConfigPage } from './pages/AIConfigPage'
import { ChannelsConfigPage } from './pages/ChannelsConfigPage'
import { SystemConfigPage } from './pages/SystemConfigPage'
import { SystemLogsPage } from './pages/SystemLogsPage'
import {
  SoulUserLayout,
  SoulUserSoulPanel,
  SoulUserUserPanel,
} from './pages/soul-user'
import { SkillsPage } from './pages/SkillsPage'
import { ToolsPage } from './pages/ToolsPage'
import { DeviceConfigLayout } from './pages/DeviceConfigLayout'
import { DisplayConfigPanel } from './pages/DisplayConfigPanel'
import { AudioConfigPanel } from './pages/AudioConfigPanel'
import { HardwareGpioPanel } from './pages/HardwareGpioPanel'
import { PlaceholderPage } from './pages/PlaceholderPage'
import { useScrollToTop } from './hooks/useScrollToTop'

function App() {
  useScrollToTop()
  const [settingsOpen, setSettingsOpen] = useState(false)

  return (
    <DeviceProvider>
      <ToastProvider>
        <UnsavedProvider>
          <ConfigProvider>
            <ErrorBoundary>
              <SettingsDrawer open={settingsOpen} onClose={() => setSettingsOpen(false)} />
            <Routes>
        <Route element={<Layout onOpenSettings={() => setSettingsOpen(true)} />}>
          <Route path="/" element={<Navigate to="/device" replace />} />
          <Route path="/config" element={<Navigate to="/ai-config" replace />} />
          <Route path="/device" element={<DevicePage />} />
          <Route path="/ai-config" element={<AIConfigPage />} />
          <Route path="/channels-config" element={<ChannelsConfigPage />} />
          <Route path="/system-config" element={<SystemConfigPage />} />
          <Route path="/device-config" element={<DeviceConfigLayout />}>
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
            </ErrorBoundary>
          </ConfigProvider>
        </UnsavedProvider>
      </ToastProvider>
    </DeviceProvider>
  )
}

export default App
