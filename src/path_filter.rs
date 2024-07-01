/*
*** This is the golang regex implementation
* longtailutils/pathfilter.go
type regexPathFilter struct {
    includeRegexes         []string
    compiledIncludeRegexes []*regexp.Regexp
    excludeRegexes         []string
    compiledExcludeRegexes []*regexp.Regexp
}

// MakeRegexPathFilter ...
func MakeRegexPathFilter(includeFilterRegEx string, excludeFilterRegEx string) (longtaillib.Longtail_PathFilterAPI, error) {
    const fname = "MakeRegexPathFilter"
    log := log.WithContext(context.Background()).WithFields(log.Fields{
        "fname":              fname,
        "includeFilterRegEx": includeFilterRegEx,
        "excludeFilterRegEx": excludeFilterRegEx,
    })
    log.Debug(fname)

    regexPathFilter := &regexPathFilter{}
    if includeFilterRegEx != "" {
        includeRegexes, compiledIncludeRegexes, err := splitRegexes(includeFilterRegEx)
        if err != nil {
            return longtaillib.Longtail_PathFilterAPI{}, errors.Wrap(err, fname)
        }
        regexPathFilter.includeRegexes = includeRegexes
        regexPathFilter.compiledIncludeRegexes = compiledIncludeRegexes
    }
    if excludeFilterRegEx != "" {
        excludeRegexes, compiledExcludeRegexes, err := splitRegexes(excludeFilterRegEx)
        if err != nil {
            return longtaillib.Longtail_PathFilterAPI{}, errors.Wrap(err, fname)
        }
        regexPathFilter.excludeRegexes = excludeRegexes
        regexPathFilter.compiledExcludeRegexes = compiledExcludeRegexes
    }
    if len(regexPathFilter.compiledIncludeRegexes) > 0 || len(regexPathFilter.compiledExcludeRegexes) > 0 {
        return longtaillib.CreatePathFilterAPI(regexPathFilter), nil
    }
    return longtaillib.Longtail_PathFilterAPI{}, nil
}

func (f *regexPathFilter) Include(rootPath string, assetPath string, assetName string, isDir bool, size uint64, permissions uint16) bool {
    const fname = "regexPathFilter.Include"
    log := log.WithContext(context.Background()).WithFields(log.Fields{
        "fname":       fname,
        "rootPath":    rootPath,
        "assetPath":   assetPath,
        "assetName":   assetName,
        "isDir":       isDir,
        "size":        size,
        "permissions": permissions,
    })
    log.Debug(fname)

    for i, r := range f.compiledExcludeRegexes {
        if r.MatchString(assetPath) {
            log.Debugf("Excluded file `%s` (match for `%s`)", assetPath, f.excludeRegexes[i])
            return false
        }
        if isDir {
            if r.MatchString(assetPath + "/") {
                log.Debugf("Excluded dir `%s/` (match for `%s`)", assetPath, f.excludeRegexes[i])
                return false
            }
        }
    }
    if len(f.compiledIncludeRegexes) == 0 {
        return true
    }
    for i, r := range f.compiledIncludeRegexes {
        if r.MatchString(assetPath) {
            log.Debugf("Included file `%s` (match for `%s`)", assetPath, f.includeRegexes[i])
            return true
        }
        if isDir {
            if r.MatchString(assetPath + "/") {
                log.Debugf("Included dir `%s/` (match for `%s`)", assetPath, f.includeRegexes[i])
                return true
            }
        }
    }
    log.Debugf("Skipping `%s`", assetPath)
    return false
}

func splitRegexes(regexes string) ([]string, []*regexp.Regexp, error) {
    const fname = "splitRegexes"
    log := log.WithContext(context.Background()).WithFields(log.Fields{
        "fname":   fname,
        "regexes": regexes,
    })
    log.Debug(fname)

    var regexStrings []string
    var compiledRegexes []*regexp.Regexp
    m := 0
    s := 0
    for i := 0; i < len(regexes); i++ {
        if (regexes)[i] == '\\' {
            m = -1
        } else if m == 0 && (regexes)[i] == '*' {
            m++
        } else if m == 1 && (regexes)[i] == '*' {
            r := (regexes)[s:(i - 1)]
            regex, err := regexp.Compile(r)
            if err != nil {
                return nil, nil, errors.Wrap(err, fname)
            }
            regexStrings = append(regexStrings, r)
            compiledRegexes = append(compiledRegexes, regex)
            s = i + 1
            m = 0
        } else {
            m = 0
        }
    }
    if s < len(regexes) {
        r := (regexes)[s:]
        regex, err := regexp.Compile(r)
        if err != nil {
            return nil, nil, errors.Wrap(err, fname)
        }
        regexStrings = append(regexStrings, r)
        compiledRegexes = append(compiledRegexes, regex)
    }
    return regexStrings, compiledRegexes, nil
}
*/

use regex::Regex;

use crate::PathFilterAPI;

#[repr(C)]
#[derive(Debug)]
pub struct RegexPathFilter {
    includeRegexes: Option<Vec<String>>,
    compiledIncludeRegexes: Option<Vec<Regex>>,
    excludeRegexes: Option<Vec<String>>,
    compiledExcludeRegexes: Option<Vec<Regex>>,
}

impl PathFilterAPI for RegexPathFilter {
    fn include(
        &self,
        root_path: &str,
        asset_path: &str,
        asset_name: &str,
        is_dir: bool,
        size: u64,
        permissions: u16,
    ) -> bool {
        self.include(root_path, asset_path, asset_name, is_dir, size, permissions)
    }
}

// TODO: This is a direct port from golongtail. Probably better to make this idiomatic rust at some
// point
impl RegexPathFilter {
    pub fn new(
        include_filter_regex: Option<String>,
        exclude_filter_regex: Option<String>,
    ) -> Result<RegexPathFilter, Box<dyn std::error::Error>> {
        let mut regex_path_filter = RegexPathFilter {
            includeRegexes: None,
            compiledIncludeRegexes: None,
            excludeRegexes: None,
            compiledExcludeRegexes: None,
        };
        if let Some(include_filter_regex) = include_filter_regex {
            let (include_regexes, compiled_include_regexes) =
                Self::split_regexes(&include_filter_regex)?;
            regex_path_filter.includeRegexes = Some(include_regexes);
            regex_path_filter.compiledIncludeRegexes = Some(compiled_include_regexes);
        }
        if let Some(exclude_filter_regex) = exclude_filter_regex {
            let (exclude_regexes, compiled_exclude_regexes) =
                Self::split_regexes(&exclude_filter_regex)?;
            regex_path_filter.excludeRegexes = Some(exclude_regexes);
            regex_path_filter.compiledExcludeRegexes = Some(compiled_exclude_regexes);
        }
        Ok(regex_path_filter)
    }

    fn split_regexes(
        regexes: &str,
    ) -> Result<(Vec<String>, Vec<Regex>), Box<dyn std::error::Error>> {
        let mut regex_strings = Vec::new();
        let mut compiled_regexes = Vec::new();
        let mut m = 0;
        let mut s = 0;
        for (i, c) in regexes.chars().enumerate() {
            if c == '\\' {
                m = -1;
            } else if m == 0 && c == '*' {
                m += 1;
            } else if m == 1 && c == '*' {
                let r = &regexes[s..i - 1];
                let regex = Regex::new(r)?;
                regex_strings.push(r.to_string());
                compiled_regexes.push(regex);
                s = i + 1;
                m = 0;
            } else {
                m = 0;
            }
        }
        if s < regexes.len() {
            let r = &regexes[s..];
            let regex = Regex::new(r)?;
            regex_strings.push(r.to_string());
            compiled_regexes.push(regex);
        }
        Ok((regex_strings, compiled_regexes))
    }

    pub fn include(
        &self,
        _root_path: &str,
        asset_path: &str,
        _asset_name: &str,
        is_dir: bool,
        _size: u64,
        _permissions: u16,
    ) -> bool {
        if let Some(compiled_exclude_regexes) = &self.compiledExcludeRegexes {
            for (i, r) in compiled_exclude_regexes.iter().enumerate() {
                if r.is_match(asset_path) {
                    println!(
                        "Excluded file `{}` (match for `{}`)",
                        asset_path,
                        self.excludeRegexes.as_ref().unwrap().get(i).unwrap()
                    );
                    return false;
                }
                if is_dir && r.is_match(&format!("{}/", asset_path)) {
                    println!(
                        "Excluded dir `{}/` (match for `{}`)",
                        asset_path,
                        self.excludeRegexes.as_ref().unwrap().get(i).unwrap()
                    );
                    return false;
                }
            }
        }
        if self.compiledIncludeRegexes.is_none() {
            return true;
        }
        if let Some(compiled_include_regexes) = &self.compiledIncludeRegexes {
            for (i, r) in compiled_include_regexes.iter().enumerate() {
                if r.is_match(asset_path) {
                    println!(
                        "Included file `{}` (match for `{}`)",
                        asset_path,
                        self.includeRegexes.as_ref().unwrap().get(i).unwrap()
                    );
                    return true;
                }
                if is_dir && r.is_match(&format!("{}/", asset_path)) {
                    println!(
                        "Included dir `{}/` (match for `{}`)",
                        asset_path,
                        self.includeRegexes.as_ref().unwrap().get(i).unwrap()
                    );
                    return true;
                }
            }
        }
        println!("Skipping `{}`", asset_path);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_regex_path_filter() {
        let regex_path_filter =
            RegexPathFilter::new(
            Some(r".*\.txt$".to_string()), 
            Some(r".*\.rs$".to_string())
        ).unwrap();
        assert!(regex_path_filter.include("root", "file.txt", "", false, 0, 0));
        assert!(!regex_path_filter.include("root", "file.rs", "", false, 0, 0));
    }
}
