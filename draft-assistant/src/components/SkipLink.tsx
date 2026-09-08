// The first thing in the tab order: a way past the header to the board.
//
// The shell rendered a `<header>` and then plain `<div>`s, with no `<main>`
// anywhere, so a screen-reader user had one landmark for the whole app and no
// way to get to the board except by tabbing through the league switcher, the
// screen toggle, the re-pull, the undo, the chime, Ask AI and the settings
// gear. The link is off screen until it takes focus, which is the only time
// anybody wants to see it.

export function SkipLink({ target, children }: { target: string; children: string }) {
  return (
    <a className="skip-link" href={`#${target}`}>
      {children}
    </a>
  );
}
