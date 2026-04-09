import { EditorPanel } from '@/components/omni/editor-panel';
import { PreviewPanel } from '@/components/omni/preview-panel';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@/components/ui/resizable';

export default function Home() {
  return (
    <ResizablePanelGroup direction="horizontal" className="h-full">
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
  );
}
