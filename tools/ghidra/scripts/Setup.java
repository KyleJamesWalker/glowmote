import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.nio.file.*;

// preScript: builds bd19 memory map for an app.bin loaded at 0x01e00120.
// args: <romdir with rom.bin, syms.txt> <ram_data_lma> <ram_data_vma> <len> [<lma> <vma> <len> ...]
public class Setup extends GhidraScript {
  public void run() throws Exception {
    String[] a = getScriptArgs();
    Memory m = currentProgram.getMemory();
    byte[] rom = Files.readAllBytes(Paths.get(a[0], "rom.bin"));
    MemoryBlock rb = m.createInitializedBlock("maskrom", toAddr(0x100000), rom.length, (byte)0, monitor, false);
    m.setBytes(toAddr(0x100000), rom);
    rb.setWrite(false); rb.setExecute(true);
    MemoryBlock ram = m.createInitializedBlock("ram", toAddr(0), 0x10000, (byte)0, monitor, false);
    ram.setWrite(true); ram.setExecute(true);
    for (int i = 1; i + 2 < a.length; i += 3) {
      long lma = Long.decode(a[i]), vma = Long.decode(a[i+1]); int len = Integer.decode(a[i+2]);
      byte[] b = new byte[len]; m.getBytes(toAddr(lma), b);
      m.setBytes(toAddr(vma), b);
      println(String.format("copied %x -> %x len %x", lma, vma, len));
    }
    SymbolTable st = currentProgram.getSymbolTable();
    for (String line : Files.readAllLines(Paths.get(a[0], "syms.txt"))) {
      String[] f = line.trim().split(" ");
      Address ad = toAddr(Long.parseLong(f[0], 16));
      st.createLabel(ad, f[1], SourceType.IMPORTED);
      if (!f[1].startsWith(".")) { disassemble(ad); createFunction(ad, f[1]); }
    }
    Address e = toAddr(0x01e00120);
    st.createLabel(e, "_start", SourceType.IMPORTED); st.addExternalEntryPoint(e);
    disassemble(e); createFunction(e, "_start");
  }
}
