import ghidra.app.script.GhidraScript;
import ghidra.app.util.PseudoDisassembler;
import ghidra.app.util.PseudoInstruction;
import ghidra.program.model.address.*;
import java.io.*;
import java.util.*;

// args: <tsv in: addr hex text> <out file>  ; pseudo-disassembles every listed address, writes ghidra text + len
public class RomCheck extends GhidraScript {
  public void run() throws Exception {
    String[] a = getScriptArgs();
    BufferedReader br = new BufferedReader(new FileReader(a[0]));
    PrintWriter pw = new PrintWriter(new FileWriter(a[1]));
    PseudoDisassembler pd = new PseudoDisassembler(currentProgram);
    String line; int ok=0, bad=0, fail=0;
    while ((line = br.readLine()) != null) {
      String[] f = line.split("\t");
      Address ad = toAddr(Long.parseLong(f[0],16));
      int explen = f[1].length()/2;
      String g; int glen;
      try { PseudoInstruction pi = pd.disassemble(ad); g = pi.toString(); glen = pi.getLength(); }
      catch (Exception e) { g = "!!" + e.getClass().getSimpleName(); glen = 0; }
      String st = glen==0 ? "FAIL" : (glen==explen ? "ok" : "LEN");
      if (st.equals("ok")) ok++; else if (st.equals("LEN")) bad++; else fail++;
      pw.println(st + "\t" + f[0] + "\t" + f[1] + "\t" + f[2] + "\t|\t" + g);
    }
    pw.close();
    println("ok="+ok+" len_mismatch="+bad+" fail="+fail);
  }
}
