import { ChevronLeft, ChevronRight } from "lucide-react";
import { DayPicker } from "react-day-picker";

/** shadcn-style calendar: react-day-picker restyled with the app's design
 * tokens, so it follows every theme (unlike the native date popup). */
export default function Calendar({
  selected,
  onSelect,
}: {
  selected?: Date;
  onSelect: (d?: Date) => void;
}) {
  return (
    <DayPicker
      mode="single"
      selected={selected}
      onSelect={onSelect}
      defaultMonth={selected}
      showOutsideDays
      weekStartsOn={1}
      components={{
        Chevron: ({ orientation }) =>
          orientation === "left" ? <ChevronLeft size={15} /> : <ChevronRight size={15} />,
      }}
      classNames={{
        root: "select-none p-3 text-sm text-text",
        months: "relative",
        month_caption: "flex h-8 items-center px-1.5 text-sm font-medium",
        caption_label: "text-text",
        nav: "absolute right-0 top-0 z-10 flex gap-1",
        button_previous: "rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-text",
        button_next: "rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-text",
        month_grid: "mt-2 border-collapse",
        weekday: "h-8 w-8 text-center text-xs font-normal text-faint",
        day: "p-0 text-center",
        day_button: "h-8 w-8 rounded-md text-sm hover:bg-surface-2",
        today: "font-semibold text-accent",
        selected:
          "[&>button]:bg-accent [&>button]:font-semibold [&>button]:text-on-accent [&>button]:hover:bg-accent-hover",
        outside: "text-faint/50",
        disabled: "opacity-40",
      }}
    />
  );
}
