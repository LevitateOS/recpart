const message = [
  "recpart frontend TUI has been removed.",
  "Use backend commands directly:",
  "  recpart plan --disk <disk> [--mode ab|mutable] [--json]",
  "  recpart apply --disk <disk> [--mode ab|mutable] [--dry-run|--confirm DESTROY] [--json]",
].join("\n");

console.error(message);
process.exit(2);
