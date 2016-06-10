Version history
===============

This library adheres to `Semantic Versioning <http://semver.org/>`_.

**2.0.0** (2016-06-11)

- **BACKWARD INCOMPATIBLE** Deserialize unknown tags as ``CBORTag`` objects so as not to lose
  information
- Fixed error messages coming from nested structures

**1.1.0** (2016-06-10)

- Fixed deserialization of cyclic structures

**1.0.0** (2016-06-08)

- Initial release
