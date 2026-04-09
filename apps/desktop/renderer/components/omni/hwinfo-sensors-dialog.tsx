import { useState, useMemo } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Search, Copy } from 'lucide-react';
import { useOmniState } from '@/hooks/use-omni-state';
import { useSensorData } from '@/hooks/use-sensor-data';

interface HwInfoSensorsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function HwInfoSensorsDialog({ open, onOpenChange }: HwInfoSensorsDialogProps) {
  const { state } = useOmniState();
  const sensorData = useSensorData();
  const [search, setSearch] = useState('');
  const [copied, setCopied] = useState<string | null>(null);

  // Build a merged list: metadata (path, label, unit) + live value
  const sensors = useMemo(() => {
    const valueMap = new Map<string, number>();
    if (sensorData?.hwinfo?.values) {
      for (const { path, value } of sensorData.hwinfo.values) {
        valueMap.set(path, value);
      }
    }
    return state.hwinfoSensors.map((s) => ({
      ...s,
      value: valueMap.get(s.path),
    }));
  }, [state.hwinfoSensors, sensorData?.hwinfo?.values]);

  const filtered = useMemo(() => {
    if (!search) return sensors;
    const q = search.toLowerCase();
    return sensors.filter(
      (s) =>
        s.path.toLowerCase().includes(q) ||
        s.label.toLowerCase().includes(q) ||
        s.unit.toLowerCase().includes(q),
    );
  }, [sensors, search]);

  // Group by category (first segment after hwinfo.)
  const grouped = useMemo(() => {
    const groups = new Map<string, typeof filtered>();
    for (const s of filtered) {
      const cat = s.path.split('.')[1] || 'other';
      if (!groups.has(cat)) groups.set(cat, []);
      groups.get(cat)!.push(s);
    }
    return groups;
  }, [filtered]);

  const handleCopy = (path: string) => {
    navigator.clipboard.writeText(`{${path}}`);
    setCopied(path);
    setTimeout(() => setCopied(null), 1500);
  };

  const formatValue = (value: number | undefined, unit: string) => {
    if (value === undefined || value === null) return 'N/A';
    if (unit === '°C' || unit === '°F' || unit === '%' || unit === 'W' || unit === 'MHz' || unit === 'RPM') {
      return Math.round(value).toString();
    }
    if (unit === 'V' || unit === 'A') {
      return value.toFixed(2);
    }
    return Number.isInteger(value) ? value.toString() : value.toFixed(1);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="bg-[#18181B] border-[#27272A] p-0 gap-0 overflow-hidden sm:max-w-3xl">
        <DialogHeader className="px-5 pt-5 pb-3">
          <DialogTitle className="text-[#FAFAFA] flex items-center gap-2">
            HWiNFO Sensors
            <span className="text-[10px] text-[#52525B] font-normal">
              {sensors.length} sensors
            </span>
          </DialogTitle>
        </DialogHeader>

        {/* Search */}
        <div className="px-4 pb-2">
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-[#52525B]" />
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search sensors by path, label, or unit..."
              className="pl-9 bg-[#0D0D0F] border-[#27272A] text-[#FAFAFA] h-8 text-xs"
            />
          </div>
        </div>

        <div className="border-b border-[#27272A]" />

        {/* Sensor list */}
        <div className="max-h-[500px] overflow-y-auto">
          {[...grouped.entries()].map(([category, items]) => (
            <div key={category}>
              <div className="sticky top-0 bg-[#18181B] px-4 py-1.5 border-b border-[#27272A]">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-[#F59E0B]">
                  {category}
                </span>
                <span className="text-[10px] text-[#52525B] ml-2">{items.length}</span>
              </div>
              <div className="px-2 py-1">
                {items.map((s, idx) => (
                  <div
                    key={`${s.path}-${idx}`}
                    className="group flex items-center gap-3 px-2 py-1 rounded hover:bg-[#27272A] transition-colors cursor-pointer"
                    onClick={() => handleCopy(s.path)}
                    title={`Click to copy {${s.path}}`}
                  >
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-[11px] text-[#FAFAFA] truncate">
                          {s.path}
                        </span>
                        <Copy
                          className={`h-3 w-3 shrink-0 transition-colors ${
                            copied === s.path
                              ? 'text-[#22C55E]'
                              : 'text-[#52525B] opacity-0 group-hover:opacity-100'
                          }`}
                        />
                      </div>
                      <span className="text-[10px] text-[#71717A] truncate block">
                        {s.label}
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5 shrink-0">
                      <span className="font-mono text-xs text-[#F59E0B] min-w-[60px] text-right">
                        {formatValue(s.value, s.unit)}
                      </span>
                      <span className="text-[10px] text-[#52525B] min-w-[30px]">{s.unit}</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
          {filtered.length === 0 && (
            <div className="text-center text-[#52525B] text-xs py-8">No sensors found</div>
          )}
        </div>

        {/* Footer */}
        <div className="px-4 py-2.5 border-t border-[#27272A] flex items-center justify-between">
          <span className="text-[10px] text-[#52525B]">
            Click a sensor to copy its path as a template variable
          </span>
          <span className="text-[10px] text-[#52525B]">
            {filtered.length} / {sensors.length}
          </span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
