import { Radio } from 'lucide-react';
import type { SensorSnapshot } from '@/generated/SensorSnapshot';
import type { HwInfoData } from '@/lib/sensor-mapping';
import { useOmniState } from '@/hooks/use-omni-state';

interface SensorEntry {
  label: string;
  value: string;
  unit: string;
  category: 'frame' | 'cpu' | 'gpu' | 'ram';
}

const categoryColors = {
  frame: '#00D9FF',
  cpu: '#3B82F6',
  gpu: '#A855F7',
  ram: '#22C55E',
};

function fmt(v: unknown, decimals = 0): string {
  if (v == null) return 'N/A';
  const n = Number(v);
  if (isNaN(n)) return 'N/A';
  return n.toFixed(decimals);
}

function buildEntries(s: SensorSnapshot): SensorEntry[] {
  const entries: SensorEntry[] = [];

  // Frame
  if (s.frame.available) {
    entries.push({ label: 'FPS', value: fmt(s.frame.fps), unit: '', category: 'frame' });
    entries.push({
      label: 'Frame Time',
      value: fmt(s.frame.frame_time_ms, 1),
      unit: 'ms',
      category: 'frame',
    });
    entries.push({
      label: 'Frame Avg',
      value: fmt(s.frame.frame_time_avg_ms, 1),
      unit: 'ms',
      category: 'frame',
    });
    entries.push({
      label: 'Frame 1%',
      value: fmt(s.frame.frame_time_1percent_ms, 1),
      unit: 'ms',
      category: 'frame',
    });
    entries.push({
      label: 'Frame 0.1%',
      value: fmt(s.frame.frame_time_01percent_ms, 1),
      unit: 'ms',
      category: 'frame',
    });
  } else {
    entries.push({ label: 'FPS', value: '—', unit: '', category: 'frame' });
    entries.push({ label: 'Frame Time', value: '—', unit: '', category: 'frame' });
  }

  // CPU
  entries.push({
    label: 'CPU Usage',
    value: fmt(s.cpu.total_usage_percent),
    unit: '%',
    category: 'cpu',
  });
  entries.push({
    label: 'CPU Temp',
    value: fmt(s.cpu.package_temp_c),
    unit: '°C',
    category: 'cpu',
  });

  // GPU
  entries.push({ label: 'GPU Usage', value: fmt(s.gpu.usage_percent), unit: '%', category: 'gpu' });
  entries.push({ label: 'GPU Temp', value: fmt(s.gpu.temp_c), unit: '°C', category: 'gpu' });
  entries.push({
    label: 'GPU Clock',
    value: fmt(s.gpu.core_clock_mhz),
    unit: 'MHz',
    category: 'gpu',
  });
  entries.push({
    label: 'Mem Clock',
    value: fmt(s.gpu.mem_clock_mhz),
    unit: 'MHz',
    category: 'gpu',
  });
  entries.push({
    label: 'VRAM',
    value: `${fmt(s.gpu.vram_used_mb)}/${fmt(s.gpu.vram_total_mb)}`,
    unit: 'MB',
    category: 'gpu',
  });
  entries.push({ label: 'GPU Power', value: fmt(s.gpu.power_draw_w), unit: 'W', category: 'gpu' });
  entries.push({
    label: 'GPU Fan',
    value: fmt(s.gpu.fan_speed_percent),
    unit: '%',
    category: 'gpu',
  });

  // RAM
  entries.push({ label: 'RAM Usage', value: fmt(s.ram.usage_percent), unit: '%', category: 'ram' });
  entries.push({ label: 'RAM Used', value: s.ram.used_mb.toString(), unit: 'MB', category: 'ram' });
  entries.push({
    label: 'RAM Total',
    value: s.ram.total_mb.toString(),
    unit: 'MB',
    category: 'ram',
  });

  return entries;
}

interface Props {
  snapshot: SensorSnapshot;
  hwinfo?: HwInfoData;
}

export function SensorReadout({ snapshot, hwinfo }: Props) {
  const { state } = useOmniState();
  const entries = buildEntries(snapshot);

  return (
    <div className="p-3">
      <div className="flex items-center gap-2 mb-3">
        <Radio className="h-3.5 w-3.5 text-[#22C55E]" />
        <h3 className="text-[10px] font-semibold uppercase tracking-widest text-[#71717A]">
          Live Sensors
        </h3>
      </div>
      <div className="grid grid-cols-2 gap-x-4 gap-y-1">
        {entries.map((entry) => (
          <div key={entry.label} className="flex items-center justify-between py-0.5">
            <span className="text-[10px] text-[#71717A] uppercase tracking-wide">
              {entry.label}
            </span>
            <span
              className="text-xs font-mono font-medium"
              style={{ color: categoryColors[entry.category] }}
            >
              {entry.value}
              {entry.unit && <span className="text-[#52525B] ml-0.5">{entry.unit}</span>}
            </span>
          </div>
        ))}
      </div>
      {state.hwinfoConnected && state.hwinfoSensorCount > 0 && (
        <div className="mt-2">
          <div className="flex items-center gap-2">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-[#F59E0B]">
              HWiNFO
            </span>
            <span className="text-[10px] text-[#52525B]">
              {state.hwinfoSensorCount} sensors streaming
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
