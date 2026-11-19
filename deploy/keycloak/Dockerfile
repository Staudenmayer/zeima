# Keycloak with the Zeima event listener (keycloak/listener): the standard image plus one JAR in
# providers/, plus the theme and the realm.
FROM docker.io/library/maven:3.9-eclipse-temurin-21@sha256:80034909d9a9f0b89d3d2c583f07f3b7650309bc4e6b886ad5b860b9e1bfa439 AS listener
WORKDIR /build
COPY listener/pom.xml .
RUN mvn -q -B dependency:go-offline
COPY listener/src src
RUN mvn -q -B package -DskipTests

FROM quay.io/keycloak/keycloak:26.8.1@sha256:3f3c5f8e03c614774b4bd9173b88391fefa4153bfc010d8012d59ad825377332
COPY --from=listener /build/target/zeima-keycloak-listener.jar /opt/keycloak/providers/
# The theme and the realm are part of the image, so it also runs where nothing can be mounted
# (Kubernetes, the air-gapped bundle). The compose files mount the same files over them, which
# lets a change take effect without a rebuild during development.
COPY themes/zeima /opt/keycloak/themes/zeima
COPY realm-zeima.json /opt/keycloak/data/import/realm-zeima.json
