import { Loader2 } from 'lucide-react';

export function SplashLoader() {
  return (
    <div
      role="status"
      aria-label="Loading"
      className="flex h-screen w-screen items-center justify-center bg-[#0d0d0f]"
    >
      <Loader2 className="h-6 w-6 animate-spin text-[#00d9ff]" />
    </div>
  );
}
