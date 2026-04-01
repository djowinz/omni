import { OmniProvider } from '@/hooks/use-omni-state';
import { Header } from '@/components/omni/header';
import { StatusBar } from '@/components/omni/status-bar';
import { WidgetPanel } from '@/components/omni/widget-panel';
import { EditorPanel } from '@/components/omni/editor-panel';
import { PreviewPanel } from '@/components/omni/preview-panel';
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from '@/components/ui/resizable';

export default function Home() {
  return (
    <OmniProvider>
      <div className="flex h-screen flex-col bg-[#0D0D0F] text-[#FAFAFA]">
        <Header />
        <main className="flex-1 overflow-hidden">
          <ResizablePanelGroup direction="horizontal" className="h-full">
            <ResizablePanel defaultSize={18} minSize={15} maxSize={25}>
              <WidgetPanel />
            </ResizablePanel>
            <ResizableHandle
              withHandle
              className="w-1 bg-[#0D0D0F] hover:bg-[#00D9FF]/30 transition-colors data-[resize-handle-active]:bg-[#00D9FF]/50"
            />
            <ResizablePanel defaultSize={47} minSize={30}>
              <EditorPanel />
            </ResizablePanel>
            <ResizableHandle
              withHandle
              className="w-1 bg-[#0D0D0F] hover:bg-[#00D9FF]/30 transition-colors data-[resize-handle-active]:bg-[#00D9FF]/50"
            />
            <ResizablePanel defaultSize={35} minSize={25}>
              <PreviewPanel />
            </ResizablePanel>
          </ResizablePanelGroup>
        </main>
        <StatusBar />
      </div>
    </OmniProvider>
  );
}
