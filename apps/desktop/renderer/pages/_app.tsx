import { useState, type ReactNode } from 'react';
import type { AppProps } from 'next/app';
import { Toaster } from 'sonner';
import { NuqsAdapter } from 'nuqs/adapters/next/pages';
import '../styles/globals.css';
import { OmniProvider } from '@/hooks/use-omni-state';
import { Header } from '@/components/omni/header';
import { StatusBar } from '@/components/omni/status-bar';
import { WidgetPanel } from '@/components/omni/widget-panel';
import { SettingsPanel } from '@/components/omni/settings-panel';
import { ActivityBar } from '@/components/omni/activity-bar';
import { ExplorePanel } from '@/components/omni/explore-panel';
import { useOmniState } from '@/hooks/use-omni-state';
import { PreviewContextProvider } from '../lib/preview-context';
import { PreviewBanner } from '../components/omni/preview-banner';
import { IdentityContextProvider, useIdentity } from '../lib/identity-context';
import { SplashLoader } from '../components/omni/splash-loader';
import { IdentityWelcomeDialog } from '../components/omni/identity-welcome-dialog';
import { IdentityBackupDialog } from '../components/omni/identity-backup-dialog';

function IdentityGate({ children }: { children: ReactNode }) {
  const { identity, loading, is_fresh_install, markFirstRunHandled, refresh } = useIdentity();
  const [importOpen, setImportOpen] = useState(false);

  if (loading || identity === null) return <SplashLoader />;

  return (
    <>
      <IdentityWelcomeDialog
        open={is_fresh_install && !importOpen}
        onSetUpNew={markFirstRunHandled}
        onImport={() => setImportOpen(true)}
      />
      <IdentityBackupDialog
        open={importOpen}
        mode="import"
        lockOverwrite
        onOpenChange={setImportOpen}
        onSuccess={() => {
          setImportOpen(false);
          markFirstRunHandled();
          void refresh();
        }}
      />
      {children}
    </>
  );
}

function AppLayout({ children }: { children: React.ReactNode }) {
  const { state } = useOmniState();

  if (state.activePanel === 'explore') {
    return (
      <div className="flex h-screen flex-col bg-[#0D0D0F] text-[#FAFAFA]">
        <PreviewBanner />
        <Header />
        <main className="flex-1 overflow-hidden flex">
          <ActivityBar />
          <div className="flex-1 overflow-hidden">
            <ExplorePanel />
          </div>
        </main>
        <StatusBar />
      </div>
    );
  }

  return (
    <div className="flex h-screen flex-col bg-[#0D0D0F] text-[#FAFAFA]">
      <PreviewBanner />
      <Header />
      <main className="flex-1 overflow-hidden flex">
        <ActivityBar />
        <div className="w-72 flex-shrink-0 border-r border-[#27272A]">
          {state.activePanel === 'components' ? <WidgetPanel /> : <SettingsPanel />}
        </div>
        <div className="flex-1 overflow-hidden">{children}</div>
      </main>
      <StatusBar />
    </div>
  );
}

export default function App({ Component, pageProps }: AppProps) {
  return (
    <div className="dark">
      <IdentityContextProvider>
        <IdentityGate>
          <NuqsAdapter>
            <OmniProvider>
              <PreviewContextProvider>
                <AppLayout>
                  <Toaster richColors position="bottom-right" />
                  <Component {...pageProps} />
                </AppLayout>
              </PreviewContextProvider>
            </OmniProvider>
          </NuqsAdapter>
        </IdentityGate>
      </IdentityContextProvider>
    </div>
  );
}
