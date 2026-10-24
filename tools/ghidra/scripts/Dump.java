import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import ghidra.program.model.address.*;
import ghidra.program.model.mem.*;
import ghidra.program.model.symbol.*;
import java.io.*;

public class Dump extends GhidraScript {
  public void run() throws Exception {
    String out = getScriptArgs()[0];
    Listing l = currentProgram.getListing();
    PrintWriter pw = new PrintWriter(new FileWriter(out));
    long code=0;
    InstructionIterator it = l.getInstructions(true);
    while (it.hasNext()) {
      Instruction ins = it.next();
      code += ins.getLength();
      Function f = getFunctionAt(ins.getAddress());
      if (f != null) pw.println("\n;---- FUNC " + f.getName());
      StringBuilder sb = new StringBuilder();
      for (byte b : ins.getBytes()) sb.append(String.format("%02x", b));
      String refs="";
      for (Reference r : ins.getReferencesFrom()) refs += " ->" + r.getToAddress();
      pw.println(ins.getAddress() + "  " + String.format("%-12s", sb) + " " + ins.toString() + refs);
    }
    pw.close();
    println("code bytes: " + code + " funcs: " + currentProgram.getFunctionManager().getFunctionCount());
    java.util.Iterator<Bookmark> bi = currentProgram.getBookmarkManager().getBookmarksIterator("Error");
    int n=0; while (bi.hasNext()) { Bookmark b = bi.next(); if (n++<40) println("ERR " + b.getAddress() + " " + b.getComment()); }
    println("errors: " + n);
  }
}
