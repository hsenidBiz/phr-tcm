import { Calendar as XiodCalendar } from "xiod-ui/calendar";

/**
 * The date grid DateField opens: XiodUI's calendar, weeks starting on
 * Monday, coloured by the app's tokens through src/xiod-theme.css. Single
 * dates only - that is all an Azure DevOps date field holds. Its own footer
 * carries "Today". It lays itself flat (no card of its own) because the
 * panel it sits in is the card.
 */
export default function Calendar({
  selected,
  onSelect,
}: {
  selected?: Date;
  onSelect: (d?: Date) => void;
}) {
  return (
    <XiodCalendar
      mode="single"
      selected={selected ?? null}
      onSelect={(d) => onSelect(d instanceof Date ? d : undefined)}
      weekStartsOn={1}
      showOutsideDays
      className="border-none bg-transparent shadow-none before:shadow-none!"
    />
  );
}
