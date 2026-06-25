// Force line/block flushing when Node LSP servers write to a pipe (not a TTY).
// Without this, responses can stall in stdout buffer and the IDE never reads them.
if (process.stdout && typeof process.stdout._handle?.setBlocking === 'function') {
  process.stdout._handle.setBlocking(true);
}
if (process.stderr && typeof process.stderr._handle?.setBlocking === 'function') {
  process.stderr._handle.setBlocking(true);
}
