// Differential oracle: runs an ops file through Java ZetaSketch and prints one line per EMIT.
//
//   javac -cp zetasketch-all.jar Oracle.java
//   java -cp zetasketch-all.jar:. Oracle < ops.txt > java.txt
//
// Ops (one per line, fields separated by single spaces):
//   S <p> <sp> <L|S>   start a new current sketch for longs (L) or strings (S)
//   A <value>          add: a decimal i64 for L, hex-encoded UTF-8 bytes for S ("-" = "")
//   M <base64>         merge serialized sketch bytes into the current sketch
//   E                  emit: "<base64 of serializeToByteArray()> <longResult()>"
// Any exception prints "EXC <class>" for that emit and skips ops until the next S.
// The Rust side (examples/oracle_driver.rs) writes the same format.

import com.google.zetasketch.HyperLogLogPlusPlus;
import java.io.BufferedReader;
import java.io.InputStreamReader;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.util.Base64;

public final class Oracle {
  private Oracle() {}

  private static String fromHex(String hex) {
    if (hex.equals("-")) {
      return "";
    }
    byte[] b = new byte[hex.length() / 2];
    for (int i = 0; i < b.length; i++) {
      b[i] = (byte) Integer.parseInt(hex.substring(2 * i, 2 * i + 2), 16);
    }
    return new String(b, StandardCharsets.UTF_8);
  }

  @SuppressWarnings({"unchecked", "rawtypes"})
  public static void main(String[] args) throws Exception {
    BufferedReader in =
        new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
    PrintWriter out = new PrintWriter(System.out);
    HyperLogLogPlusPlus current = null;
    boolean strings = false;
    boolean failed = false;
    String failure = "";
    int lineNo = 0;
    String line;
    while ((line = in.readLine()) != null) {
      lineNo++;
      if (line.isEmpty()) {
        continue;
      }
      String[] f = line.split(" ");
      try {
        switch (f[0]) {
          case "S":
            failed = false;
            HyperLogLogPlusPlus.Builder b =
                new HyperLogLogPlusPlus.Builder()
                    .normalPrecision(Integer.parseInt(f[1]))
                    .sparsePrecision(Integer.parseInt(f[2]));
            strings = f[3].equals("S");
            current = strings ? b.buildForStrings() : b.buildForLongs();
            break;
          case "A":
            if (failed) {
              break;
            }
            if (strings) {
              ((HyperLogLogPlusPlus) current).add((Object) fromHex(f[1]));
            } else {
              current.add(Long.parseLong(f[1]));
            }
            break;
          case "M":
            if (failed) {
              break;
            }
            current.merge(Base64.getDecoder().decode(f[1]));
            break;
          case "E":
            if (failed) {
              out.println("EXC " + failure);
              break;
            }
            byte[] bytes = current.serializeToByteArray();
            out.println(Base64.getEncoder().encodeToString(bytes) + " " + current.longResult());
            break;
          default:
            throw new IllegalArgumentException("unknown op " + f[0]);
        }
      } catch (RuntimeException e) {
        failed = true;
        failure =
            e.getClass().getSimpleName()
                + " at ops line "
                + lineNo
                + " ("
                + f[0]
                + "): "
                + String.valueOf(e.getMessage()).replace('
', ' ');
        if (f[0].equals("E")) {
          out.println("EXC " + failure);
        }
      }
    }
    out.flush();
  }
}
