import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.*;
import java.io.*;
import java.util.*;

// args: <outfile> <hexaddr>...   creates function if missing (needs RW=1 to persist) and decompiles
public class Decomp extends GhidraScript {
  public void run() throws Exception {
    String[] a = getScriptArgs();
    PrintWriter pw = new PrintWriter(new FileWriter(a[0]));
    DecompInterface d = new DecompInterface();
    d.openProgram(currentProgram);
    Set<Address> seen = new HashSet<>();
    for (int i = 1; i < a.length; i++) {
      Address ad = toAddr(Long.parseLong(a[i], 16));
      Function f = getFunctionContaining(ad);
      if (f == null) { disassemble(ad); f = createFunction(ad, null); }
      if (f == null) { pw.println("// no function at " + ad); continue; }
      if (!seen.add(f.getEntryPoint())) continue;
      DecompileResults r = d.decompileFunction(f, 120, monitor);
      pw.println("// ===== " + f.getName() + " @ " + f.getEntryPoint() + " body " + f.getBody().getMinAddress() + "-" + f.getBody().getMaxAddress());
      if (r.decompileCompleted()) pw.println(r.getDecompiledFunction().getC());
      else pw.println("// FAILED: " + r.getErrorMessage());
    }
    pw.close();
  }
}
