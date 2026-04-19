// Developer smoke harness for Share Hub Wave-3a shared primitives (#014).
// NOT part of the production user flow. Filename prefix `__` is the convention
// for internal developer pages; not linked from any user-visible navigation.
// Removable after Wave 3b wires these primitives into real flows.

import { useEffect } from 'react';

import { ArtifactCard } from '@/components/omni/artifact-card';
import { InstallProgress } from '@/components/omni/install-progress';
import PolicyDisclosure from '@/components/omni/policy-disclosure';
import { PreviewBanner } from '@/components/omni/preview-banner';
import { ExploreSidebar } from '@/components/omni/explore-sidebar';
import { ExploreEmptyState } from '@/components/omni/explore-empty-state';
import { ExploreDetail } from '@/components/omni/explore-detail';
import { PreviewContextProvider, usePreview } from '@/lib/preview-context';
import { useShareWs } from '@/hooks/use-share-ws';
import { POLICY_ALLOWED, POLICY_NOT_ALLOWED } from '@/lib/policy';
import type { CachedArtifactDetail } from '@/lib/share-types';
import { Button } from '@/components/ui/button';

const SAMPLE_ARTIFACT: CachedArtifactDetail = {
  artifact_id: '4a2e9b1c3d7e8f0a',
  content_hash: 'deadbeefcafef00d'.repeat(4),
  author_pubkey: '00'.repeat(32),
  author_fingerprint_hex: '',
  name: 'CyberHUD Neon',
  kind: 'bundle',
  tags: [],
  installs: 0,
  r2_url: 'https://example.invalid/blob/x.omnipkg',
  thumbnail_url: 'https://example.invalid/v1/thumb/4a2e9b1c3d7e8f0a',
  created_at: 0,
  updated_at: 1_700_000_000,
};

function PreviewControls() {
  const { activeToken, setPreview, clearPreview } = usePreview();
  return (
    <div className="flex flex-wrap gap-2">
      <Button
        type="button"
        onClick={() => setPreview('smoke-token-abc', SAMPLE_ARTIFACT)}
        disabled={activeToken !== null}
      >
        Activate preview
      </Button>
      <Button type="button" variant="outline" onClick={() => clearPreview()}>
        Clear preview
      </Button>
    </div>
  );
}

function UseShareWsSmoke() {
  const { send, subscribe } = useShareWs();
  // Exercise the subscribe() surface at mount — ensures it compiles against the typed registry.
  useEffect(() => {
    const unsubscribe = subscribe('explorer.installProgress', (frame) => {
      console.log('[smoke] installProgress frame', frame);
    });
    return unsubscribe;
  }, [subscribe]);
  // Exercise the send() surface — do not actually dispatch; just reference the bound function.
  void send;
  return (
    <p className="text-xs text-muted-foreground">
      <code>useShareWs()</code> wired: <code>send()</code> +{' '}
      <code>subscribe(&apos;explorer.installProgress&apos;, …)</code> mounted.
    </p>
  );
}

export default function PrimitivesSmokeShare() {
  return (
    <PreviewContextProvider>
      <PreviewBanner />
      <div className="max-w-4xl mx-auto p-8 space-y-10">
        <header className="space-y-1">
          <h1 className="text-2xl font-semibold">Share Hub primitives smoke</h1>
          <p className="text-sm text-muted-foreground">
            Developer-only page exercising every Wave-3a export (#014). Not linked from user-facing
            navigation. Complements <code>__primitives-smoke.tsx</code> (shared UI primitives from
            #020).
          </p>
        </header>

        <section className="space-y-3">
          <h2 className="text-lg font-semibold">ArtifactCard — grid variant</h2>
          <div className="grid grid-cols-3 gap-4">
            <ArtifactCard variant="grid" artifact={SAMPLE_ARTIFACT} />
            <ArtifactCard variant="grid" artifact={SAMPLE_ARTIFACT} installed />
            <ArtifactCard
              variant="grid"
              artifact={{ ...SAMPLE_ARTIFACT, kind: 'theme', name: 'Midnight Mono' }}
            />
          </div>
        </section>

        <section className="space-y-3">
          <h2 className="text-lg font-semibold">ArtifactCard — detail variant</h2>
          <ArtifactCard
            variant="detail"
            artifact={SAMPLE_ARTIFACT}
            actionSlots={{
              left: (
                <Button type="button" variant="outline">
                  Preview
                </Button>
              ),
              middle: <Button type="button">Install</Button>,
              right: (
                <Button type="button" variant="outline">
                  Fork
                </Button>
              ),
            }}
            kebabMenuItems={null}
          />
        </section>

        <section className="space-y-3">
          <h2 className="text-lg font-semibold">InstallProgress — each phase</h2>
          <div className="space-y-2">
            <InstallProgress phase="download" done={30} total={100} label="Downloading…" />
            <InstallProgress phase="verify" done={50} total={100} label="Verifying…" />
            <InstallProgress phase="sanitize" done={80} total={100} label="Sanitizing…" />
            <InstallProgress phase="write" done={95} total={100} label="Writing files…" />
            <InstallProgress phase="done" done={100} total={100} label="Installed" />
            <InstallProgress phase="error" done={42} total={100} label="Install failed" />
          </div>
        </section>

        <section className="space-y-3">
          <h2 className="text-lg font-semibold">PolicyDisclosure</h2>
          <PolicyDisclosure />
          <p className="text-xs text-muted-foreground">
            {POLICY_ALLOWED.length} allowed bullets · {POLICY_NOT_ALLOWED.length} not-allowed
            bullets.
          </p>
        </section>

        <section className="space-y-3">
          <h2 className="text-lg font-semibold">PreviewBanner + PreviewContext</h2>
          <p className="text-sm text-muted-foreground">
            Click Activate to mount the banner at the top of this page (scroll up).
          </p>
          <PreviewControls />
        </section>

        <section className="space-y-3">
          <h2 className="text-lg font-semibold">useShareWs</h2>
          <UseShareWsSmoke />
        </section>

        <section className="space-y-3 border-t border-zinc-800 pt-6">
          <h2 className="text-lg font-semibold">Wave 3b — Explore primitives</h2>
          <p className="text-sm text-muted-foreground">
            Sidebar, empty-state, and detail pane rendered standalone. Grid + full ExplorePanel need
            live <code>useShareWs</code> data, exercised via the Explore tab in dev-mode smoke.
          </p>
          <div
            className="grid grid-cols-3 gap-4 rounded-md border border-zinc-800 bg-[#0D0D0F]"
            style={{ height: 360 }}
          >
            <ExploreSidebar />
            <ExploreEmptyState
              label="Sample empty state"
              hint="This renders when a sub-tab has no content yet."
            />
            <ExploreDetail selectedId={null} tab="discover" />
          </div>
        </section>
      </div>
    </PreviewContextProvider>
  );
}
