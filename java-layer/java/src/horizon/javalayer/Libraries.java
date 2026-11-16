package horizon.javalayer;

import org.w3c.dom.Document;
import org.w3c.dom.Element;
import org.w3c.dom.Node;

import javax.xml.parsers.DocumentBuilderFactory;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.net.URI;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * plugin.yml's `libraries:` as Paper's MavenLibraryResolver resolves them: each one with its compile and runtime
 * dependencies, theirs, and so on (nearest version wins; test, provided, system and optional ones left out;
 * exclusions kept), from Maven Central into java-layer/libraries. POMs come with their parents' properties and
 * dependency management, BOMs (`import`) included.
 */
final class Libraries {
    private Libraries() {}

    static final String CENTRAL = "https://repo.maven.apache.org/maven2/";
    private static final Pattern PROPERTY = Pattern.compile("\\$\\{([^}]+)}");

    /** One artifact to resolve, with what the path to it excluded (groupId:artifactId, or groupId:*). */
    private record Want(String group, String artifact, String version, Set<String> excluded) {}

    /** A POM as resolved: its coordinates, properties, managed versions (and scopes) and dependencies. */
    private static final class Pom {
        String group, artifact, version, packaging = "jar";
        final Map<String, String> properties = new HashMap<>();
        final Map<String, String[]> managed = new HashMap<>(); // g:a -> {version, scope}
        final List<Element> dependencies = new ArrayList<>();
    }

    private static final Map<String, Pom> POMS = new HashMap<>();

    /** The jars for `coordinates` (groupId:artifactId:version), dependencies included, in resolution order. */
    static List<File> resolve(List<String> coordinates, File root, String owner) {
        Map<String, String> chosen = new LinkedHashMap<>(); // g:a -> version, first (nearest) wins
        ArrayDeque<Want> todo = new ArrayDeque<>();
        for (String c : coordinates) {
            String[] p = c.split(":");
            if (p.length >= 3) todo.add(new Want(p[0], p[1], p[2], Set.of()));
        }
        List<File> out = new ArrayList<>();
        while (!todo.isEmpty()) {
            Want w = todo.poll();
            String ga = w.group + ":" + w.artifact;
            if (chosen.containsKey(ga)) continue;
            chosen.put(ga, w.version);
            Pom pom = null;
            try {
                pom = pom(w.group, w.artifact, w.version, root, 0);
            } catch (Exception e) {
                Logs.warning(owner + ": no POM for " + ga + ":" + w.version + " (" + e.getMessage() + "): its own dependencies are left out");
            }
            if (pom == null || !"pom".equals(pom.packaging)) {
                File jar = fetch(w.group, w.artifact, w.version, "jar", root, owner);
                if (jar != null) out.add(jar);
            }
            if (pom == null) continue;
            for (Element d : pom.dependencies) {
                String g = expand(text(d, "groupId"), pom), a = expand(text(d, "artifactId"), pom);
                if (g == null || a == null) continue;
                String scope = text(d, "scope");
                String[] managed = pom.managed.get(g + ":" + a);
                if (scope == null && managed != null) scope = managed[1];
                if (scope != null && !scope.equals("compile") && !scope.equals("runtime")) continue;
                if ("true".equals(expand(text(d, "optional"), pom))) continue;
                String type = text(d, "type");
                if (type != null && !type.equals("jar") && !type.equals("bundle")) continue;
                if (text(d, "classifier") != null) continue;
                if (w.excluded.contains(g + ":" + a) || w.excluded.contains(g + ":*") || w.excluded.contains("*:*")) continue;
                String v = expand(text(d, "version"), pom);
                if (v == null && managed != null) v = managed[0];
                v = range(v);
                if (v == null || v.contains("${")) continue;
                Set<String> excluded = new HashSet<>(w.excluded);
                for (Element x : children(d, "exclusions", "exclusion")) excluded.add(text(x, "groupId") + ":" + text(x, "artifactId"));
                todo.add(new Want(g, a, v, excluded));
            }
        }
        return out;
    }

    /** A version range ([1.0,2.0) and the like): its lowest bound, as the simplest pick that exists. */
    private static String range(String v) {
        if (v == null || (!v.startsWith("[") && !v.startsWith("("))) return v;
        String inner = v.substring(1, v.length() - 1).split(",")[0].trim();
        return inner.isEmpty() ? null : inner;
    }

    private static Pom pom(String g, String a, String v, File root, int depth) throws Exception {
        String key = g + ":" + a + ":" + v;
        Pom cached = POMS.get(key);
        if (cached != null) return cached;
        if (depth > 12) return null;
        File file = fetch(g, a, v, "pom", root, null);
        if (file == null) return null;
        DocumentBuilderFactory f = DocumentBuilderFactory.newInstance();
        f.setNamespaceAware(false);
        f.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);
        Document doc = f.newDocumentBuilder().parse(file);
        Element project = doc.getDocumentElement();
        Pom pom = new Pom();
        Element parent = child(project, "parent");
        Pom up = null;
        if (parent != null) up = pom(text(parent, "groupId"), text(parent, "artifactId"), text(parent, "version"), root, depth + 1);
        if (up != null) {
            pom.properties.putAll(up.properties);
            pom.managed.putAll(up.managed);
        }
        pom.group = text(project, "groupId") != null ? text(project, "groupId") : up != null ? up.group : g;
        pom.artifact = a;
        pom.version = text(project, "version") != null ? text(project, "version") : up != null ? up.version : v;
        if (text(project, "packaging") != null) pom.packaging = text(project, "packaging");
        Element props = child(project, "properties");
        if (props != null) {
            for (Node n = props.getFirstChild(); n != null; n = n.getNextSibling()) {
                if (n instanceof Element e) pom.properties.put(e.getTagName(), e.getTextContent().trim());
            }
        }
        pom.properties.put("project.version", pom.version);
        pom.properties.put("project.groupId", pom.group);
        pom.properties.put("project.artifactId", pom.artifact);
        pom.properties.put("pom.version", pom.version);
        pom.properties.put("version", pom.version);
        if (up != null) {
            pom.properties.put("project.parent.version", up.version);
            pom.properties.put("parent.version", up.version);
        }
        for (Element d : children(project, "dependencyManagement", "dependencies", "dependency")) {
            String dg = expand(text(d, "groupId"), pom), da = expand(text(d, "artifactId"), pom), dv = expand(text(d, "version"), pom);
            if (dg == null || da == null) continue;
            if ("import".equals(text(d, "scope")) && "pom".equals(text(d, "type")) && dv != null) {
                Pom bom = pom(dg, da, dv, root, depth + 1);
                if (bom != null) bom.managed.forEach(pom.managed::putIfAbsent);
                continue;
            }
            pom.managed.put(dg + ":" + da, new String[]{dv, text(d, "scope")});
        }
        pom.dependencies.addAll(children(project, "dependencies", "dependency"));
        POMS.put(key, pom);
        return pom;
    }

    private static String expand(String s, Pom pom) {
        if (s == null) return null;
        for (int round = 0; round < 8 && s.contains("${"); round++) {
            Matcher m = PROPERTY.matcher(s);
            StringBuilder b = new StringBuilder();
            while (m.find()) {
                String value = pom.properties.get(m.group(1));
                m.appendReplacement(b, Matcher.quoteReplacement(value != null ? value : m.group(0)));
            }
            m.appendTail(b);
            if (b.toString().equals(s)) break;
            s = b.toString();
        }
        return s;
    }

    /** groupId/artifactId/version/artifactId-version.ext, kept under root after the first fetch. */
    private static File fetch(String g, String a, String v, String ext, File root, String owner) {
        String path = g.replace('.', '/') + "/" + a + "/" + v + "/" + a + "-" + v + "." + ext;
        File local = new File(root, path);
        if (local.isFile()) return local;
        try {
            local.getParentFile().mkdirs();
            if (owner != null) Logs.info("downloading library " + g + ":" + a + ":" + v + " for " + owner);
            File part = new File(local.getPath() + ".part");
            try (InputStream in = URI.create(CENTRAL + path).toURL().openStream()) {
                Files.copy(in, part.toPath(), StandardCopyOption.REPLACE_EXISTING);
            }
            Files.move(part.toPath(), local.toPath(), StandardCopyOption.REPLACE_EXISTING);
            return local;
        } catch (IOException e) {
            if (owner != null) Logs.warning(owner + ": library " + g + ":" + a + ":" + v + " could not be fetched: " + e);
            return null;
        }
    }

    private static Element child(Element e, String name) {
        for (Node n = e.getFirstChild(); n != null; n = n.getNextSibling()) {
            if (n instanceof Element c && c.getTagName().equals(name)) return c;
        }
        return null;
    }

    /** The elements at the end of a path of child names. */
    private static List<Element> children(Element e, String... path) {
        List<Element> level = List.of(e);
        for (String name : path) {
            List<Element> next = new ArrayList<>();
            for (Element l : level) {
                for (Node n = l.getFirstChild(); n != null; n = n.getNextSibling()) {
                    if (n instanceof Element c && c.getTagName().equals(name)) next.add(c);
                }
            }
            level = next;
        }
        return level;
    }

    private static String text(Element e, String name) {
        Element c = child(e, name);
        if (c == null) return null;
        String t = c.getTextContent().trim();
        return t.isEmpty() ? null : t;
    }
}
