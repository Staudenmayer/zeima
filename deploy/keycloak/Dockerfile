# Keycloak with the Zeima event listener (keycloak/listener): the standard image plus one JAR in
# providers/. The themes and the realm are mounted by the compose files, as before.
FROM docker.io/library/maven:3.9-eclipse-temurin-21 AS listener
WORKDIR /build
COPY listener/pom.xml .
RUN mvn -q -B dependency:go-offline
COPY listener/src src
RUN mvn -q -B package -DskipTests

FROM quay.io/keycloak/keycloak:26.8.1
COPY --from=listener /build/target/zeima-keycloak-listener.jar /opt/keycloak/providers/
