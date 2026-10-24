import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.*;
import ghidra.app.cmd.disassemble.DisassembleCommand;

// postScript: find functions via code pointers and push{rets} prologues in undefined flash/ram-code bytes.
// args: <text_lo> <text_hi> <ramcode_lo> <ramcode_hi>
public class Discover extends GhidraScript {
  boolean isPush(Address ad) throws Exception {
    int h = getShort(ad) & 0xffff;
    return (h & 0xfff0) == 0x0170;
  }
  boolean undef(Address ad) { return getInstructionContaining(ad) == null && getDataContaining(ad) == null; }
  boolean tryFunc(Address ad) throws Exception {
    if (!undef(ad)) return false;
    DisassembleCommand c = new DisassembleCommand(ad, null, true);
    c.applyTo(currentProgram, monitor);
    if (getInstructionAt(ad) == null) return false;
    if (getFunctionAt(ad) == null) createFunction(ad, null);
    return true;
  }
  public void run() throws Exception {
    String[] a = getScriptArgs();
    long tlo = Long.decode(a[0]), thi = Long.decode(a[1]), rlo = Long.decode(a[2]), rhi = Long.decode(a[3]);
    int total = 0;
    for (int pass = 0; pass < 6; pass++) {
      int n = 0;
      // code pointers anywhere in flash image or RAM data
      for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
        if (b.getName().equals("maskrom")) continue;
        for (Address p = b.getStart(); p.add(4).compareTo(b.getEnd()) <= 0; p = p.add(4)) {
          long v = getInt(p) & 0xffffffffL;
          boolean inText = v >= tlo && v < thi, inRam = v >= rlo && v < rhi;
          if ((inText || inRam) && (v & 1) == 0) {
            Address t = toAddr(v);
            if (undef(t) && isPush(t) && tryFunc(t)) n++;
          }
        }
      }
      // prologue scan in undefined text
      for (long v = tlo; v < thi; v += 2) {
        Address t = toAddr(v);
        if (undef(t) && isPush(t) && tryFunc(t)) n++;
      }
      for (long v = rlo; v < rhi; v += 2) {
        Address t = toAddr(v);
        if (undef(t) && isPush(t) && tryFunc(t)) n++;
      }
      println("pass " + pass + " new funcs " + n);
      total += n; if (n == 0) break;
    }
    println("total " + total + " funcs=" + currentProgram.getFunctionManager().getFunctionCount());
  }
}
