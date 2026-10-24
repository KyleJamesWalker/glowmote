import ghidra.app.script.GhidraScript;
import ghidra.app.util.PseudoDisassembler;
import ghidra.app.util.PseudoInstruction;
import ghidra.program.model.address.*;

// args: <hexaddr> <count>  linear pseudo-disassembly
public class Lin extends GhidraScript {
  public void run() throws Exception {
    String[] a = getScriptArgs();
    long ad = Long.parseLong(a[0],16); int n = Integer.parseInt(a[1]);
    PseudoDisassembler pd = new PseudoDisassembler(currentProgram);
    for (int i=0;i<n;i++) {
      Address A = toAddr(ad);
      try { PseudoInstruction pi = pd.disassemble(A);
        StringBuilder sb=new StringBuilder(); for (byte b: pi.getBytes()) sb.append(String.format("%02x",b));
        println(String.format("%08x  %-12s %s", ad, sb, pi.toString())); ad += pi.getLength(); }
      catch (Exception e) { println(String.format("%08x  %04x ??", ad, getShort(A)&0xffff)); ad += 2; }
    }
  }
}
