import { OmniProvider } from "@/hooks/use-omni-state";
import { Header } from "@/components/omni/header";
import { StatusBar } from "@/components/omni/status-bar";
import { WidgetPanel } from "@/components/omni/widget-panel";
import { EditorPanel } from "@/components/omni/editor-panel";
import { PreviewPanel } from "@/components/omni/preview-panel";
import {
  ResizablePanelGroup,
  ResizablePanel,
  ResizableHandle,
} from "@/components/ui/resizable";

export default function Home() {
  return (
    <OmniProvider>
      <div className="flex h-screen flex-col bg-[#0D0D0F] text-[#FAFAFA]">
        <Header />
        <main className="flex-1 overflow-hidden flex">
          {/* Widget panel — fixed width, not resizable */}
          <div className="w-72 flex-shrink-0 border-r border-[#27272A]">
            <WidgetPanel />
          </div>

          {/* Editor + Preview — resizable split */}
          <ResizablePanelGroup direction="horizontal" className="flex-1">
            <ResizablePanel defaultSize={55} minSize={30}>
              <EditorPanel />
            </ResizablePanel>
            <ResizableHandle
              withHandle
              className="max-w-px bg-[#27272a] hover:bg-[#00D9FF]/30 transition-colors data-[resize-handle-active]:bg-[#00D9FF]/50"
            />
            <ResizablePanel defaultSize={45} minSize={25}>
              <PreviewPanel />
            </ResizablePanel>
          </ResizablePanelGroup>
        </main>
        <StatusBar />
      </div>
    </OmniProvider>
  );
}
