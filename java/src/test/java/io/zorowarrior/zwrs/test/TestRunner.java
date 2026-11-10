package io.zorowarrior.zwrs.test;

import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;

/**
 * Runs every {@code public static void test*()} method in the test classes and
 * exits non-zero when anything fails.
 *
 * <p>Output is machine-friendly ({@code ok}/{@code FAIL} lines plus a summary) so
 * CI can assert on the exit code and log the details.</p>
 */
public final class TestRunner {

    private static final List<Class<?>> SUITES = List.of(
            JsonTest.class,
            OfflineUuidTest.class,
            ZipReaderTest.class,
            PluginYamlTest.class,
            JavaPluginHostTest.class,
            LauncherArgsTest.class,
            BridgeIntegrationTest.class);

    public static void main(String[] args) throws Exception {
        List<String> failures = new ArrayList<>();
        int run = 0;
        for (Class<?> suite : SUITES) {
            List<Method> tests = new ArrayList<>();
            for (Method method : suite.getDeclaredMethods()) {
                if (method.getName().startsWith("test")
                        && Modifier.isStatic(method.getModifiers())
                        && method.getParameterCount() == 0) {
                    tests.add(method);
                }
            }
            tests.sort(Comparator.comparing(Method::getName));
            for (Method test : tests) {
                run++;
                String label = suite.getSimpleName() + "." + test.getName();
                try {
                    test.setAccessible(true);
                    test.invoke(null);
                    System.out.println("ok   " + label);
                } catch (java.lang.reflect.InvocationTargetException error) {
                    Throwable cause = error.getCause();
                    System.out.println("FAIL " + label + ": " + cause);
                    failures.add(label + ": " + cause);
                    if (System.getenv("ZWRS_TEST_TRACE") != null) {
                        cause.printStackTrace(System.out);
                    }
                } catch (ReflectiveOperationException error) {
                    System.out.println("FAIL " + label + ": " + error);
                    failures.add(label + ": " + error);
                }
            }
        }
        System.out.println();
        if (failures.isEmpty()) {
            System.out.println("JAVA TESTS PASSED: " + run + " test(s)");
            return;
        }
        System.out.println("JAVA TESTS FAILED: " + failures.size() + " of " + run + " failed");
        for (String failure : failures) {
            System.out.println("  - " + failure);
        }
        System.exit(1);
    }
}
