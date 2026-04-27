import { AlertTriangle } from 'lucide-react';

export interface InlineErrorProps {
  message: string;
  onRetry: () => void;
  onReport?: () => void;
}

export function InlineError({ message, onRetry, onReport }: InlineErrorProps) {
  return (
    <div
      role="alert"
      className="flex flex-col gap-2 rounded-md border border-[#ef4444]/40 bg-[#ef4444]/10 p-3"
    >
      <div className="flex items-start gap-2 text-xs text-[#fca5a5]">
        <AlertTriangle className="mt-0.5 h-3 w-3 flex-shrink-0" />
        <span className="leading-relaxed">{message}</span>
      </div>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onRetry}
          className="h-7 rounded border border-[#ef4444]/40 bg-transparent px-3 text-xs text-[#ef4444] hover:bg-[#ef4444]/10"
        >
          Retry install
        </button>
        {onReport ? (
          <button
            type="button"
            onClick={onReport}
            className="text-xs text-[#71717a] underline hover:text-[#a1a1aa]"
          >
            Report issue
          </button>
        ) : null}
      </div>
    </div>
  );
}
