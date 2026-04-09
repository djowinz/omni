import type { AppProps } from 'next/app';
import '../styles/globals.css';
import { OmniProvider } from '@/hooks/use-omni-state';
import { Header } from '@/components/omni/header';
import { StatusBar } from '@/components/omni/status-bar';
import { WidgetPanel } from '@/components/omni/widget-panel';
import { SettingsPanel } from '@/components/omni/settings-panel';
import { ActivityBar } from '@/components/omni/activity-bar';
import { useOmniState } from '@/hooks/use-omni-state';

function AppLayout({ children }: { children: React.ReactNode }) {
  const { state } = useOmniState();

  return (
    <div className="flex h-screen flex-col bg-[#0D0D0F] text-[#FAFAFA]">
      <Header />
      <main className="flex-1 overflow-hidden flex">
        <ActivityBar />
        <div className="w-72 flex-shrink-0 border-r border-[#27272A]">
          {state.activePanel === 'components' ? <WidgetPanel /> : <SettingsPanel />}
        </div>
        <div className="flex-1 overflow-hidden">
          {children}
        </div>
      </main>
      <StatusBar />
    </div>
  );
}

export default function App({ Component, pageProps }: AppProps) {
  return (
    <div className="dark">
      <OmniProvider>
        <AppLayout>
          <Component {...pageProps} />
        </AppLayout>
      </OmniProvider>
    </div>
  );
}
