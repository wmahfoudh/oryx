// Badge list with conditional styling.
import { useState } from "react";

type Badge = { label: string; count: number };

export function BadgeRow({ badges }: { badges: Badge[] }) {
  const [active, setActive] = useState<string | null>(null);
  return (
    <ul className="badges">
      {badges.map((badge) => (
        <li
          key={badge.label}
          onClick={() => setActive(badge.label)}
          data-active={active === badge.label}
        >
          {badge.label}: {badge.count > 99 ? "99+" : badge.count}
        </li>
      ))}
    </ul>
  );
}
