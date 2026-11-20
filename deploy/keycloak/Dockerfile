# Keycloak with the Zeima event listener (keycloak/listener): the standard image plus one JAR in
# providers/, plus the theme and the realm.
FROM docker.io/library/maven:3.9-eclipse-temurin-26@sha256:b2c1ad85954592f9928e84327c65201f308ad9b5d8ed7d5b823717c97bf23fbb AS listener
WORKDIR /build
COPY listener/pom.xml .
RUN mvn -q -B dependency:go-offline
COPY listener/src src
RUN mvn -q -B package -DskipTests

FROM quay.io/keycloak/keycloak:26.8.2-3@sha256:7e66a96e4739239070aeaf72e4c5decf0f7f682cfc725b9f7ea2b336eaba38b3
COPY --from=listener /build/target/zeima-keycloak-listener.jar /opt/keycloak/providers/
# The theme and the realm are part of the image, so it also runs where nothing can be mounted
# (Kubernetes, the air-gapped bundle). The compose files mount the same files over them, which
# lets a change take effect without a rebuild during development.
COPY themes/zeima /opt/keycloak/themes/zeima
COPY realm-zeima.json /opt/keycloak/data/import/realm-zeima.json
