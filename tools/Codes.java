// Prints the language and country codes known to the running JDK, one line per list.
// Source for the code lists in crates/disk/src/bundles.rs.
//
// Run with any JDK 11 or newer (single-file launch):
//     java tools/Codes.java
// The first line of the output names the JDK version; record it in bundles.rs when the lists
// are updated.

import java.util.*;
import java.util.stream.*;

public class Codes {
    public static void main(String[] args) {
        System.out.println("java.version=" + System.getProperty("java.version"));
        System.out.println("ISO_LANGUAGES=" + String.join(" ", new TreeSet<>(Arrays.asList(Locale.getISOLanguages()))));
        System.out.println("ISO_COUNTRIES=" + String.join(" ", new TreeSet<>(Arrays.asList(Locale.getISOCountries()))));
        // Languages and countries that occur in the JDK's available locales
        // (adds three-letter languages such as fil and a few extra regions).
        Set<String> available = Arrays.stream(Locale.getAvailableLocales())
                .map(Locale::getLanguage).filter(s -> !s.isEmpty()).collect(Collectors.toCollection(TreeSet::new));
        System.out.println("AVAILABLE_LANGUAGES=" + String.join(" ", available));
        Set<String> availableCountries = Arrays.stream(Locale.getAvailableLocales())
                .map(Locale::getCountry).filter(s -> !s.isEmpty()).collect(Collectors.toCollection(TreeSet::new));
        System.out.println("AVAILABLE_COUNTRIES=" + String.join(" ", availableCountries));
    }
}
